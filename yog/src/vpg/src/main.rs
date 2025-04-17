#![feature(exit_status_error)]

use std::fmt::Display;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use color_eyre::eyre;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use color_eyre::eyre::WrapErr;
use serde::Deserialize;

use utils::tui::ClosablePrompt;
use utils::tui::ClosablePromptError;

#[derive(Debug)]
struct PgpassFile(Vec<PgpassEntry>);

impl PgpassFile {
    // pub fn aliases(&self) -> Vec<&str> {
    //     self.0.iter().map(|l| l.alias.as_ref()).collect()
    // }
    //
    // pub fn line_by_alias(&self, alias: &str) -> Option<&PgpassEntry> {
    //     self.0.iter().find(|l| l.alias == alias)
    // }

    pub fn save(&self, pgpass_path: &Path) -> color_eyre::Result<()> {
        let mut tmp_path = PathBuf::from(pgpass_path);
        tmp_path.set_file_name(".pgpass.tmp");
        let mut tmp_file = File::create(&tmp_path)?;
        for line in &self.0 {
            writeln!(tmp_file, "{}", line)?;
        }

        std::fs::rename(&tmp_path, pgpass_path)?;

        let file = OpenOptions::new().read(true).open(pgpass_path)?;
        let mut permissions = file.metadata()?.permissions();
        permissions.set_mode(0o600);
        file.set_permissions(permissions)?;

        Ok(())
    }
}

impl TryFrom<&Path> for PgpassFile {
    type Error = color_eyre::eyre::Error;

    fn try_from(pgpass_path: &Path) -> Result<Self, Self::Error> {
        let file_content = std::fs::read_to_string(pgpass_path)?;
        let mut file_lines = file_content.lines().peekable();
        let mut pgpass_entries = vec![];

        while let Some(line) = file_lines.peek() {
            if line.is_empty() {
                file_lines.next();
            }
            match (file_lines.next(), file_lines.next()) {
                (Some(metadata_line), Some(content_line)) => {
                    let metadata = metadata_line
                        .strip_prefix('#')
                        .and_then(|s| s.split_once(' '))
                        .map(|(alias, vault_path)| Metadata {
                            alias: alias.to_string(),
                            vault_path: vault_path.to_string(),
                        })
                        .ok_or_else(|| eyre!("foo"))?;

                    let content = match content_line.split(':').collect::<Vec<_>>().as_slice() {
                        [host, port, db, user, pwd] => Content {
                            host: host.to_string(),
                            port: port.parse().context(format!(
                                "unexpected port value, found {port}, required i32"
                            ))?,
                            db: db.to_string(),
                            user: user.to_string(),
                            pwd: pwd.to_string(),
                        },
                        unexpected => {
                            bail!("unexpected split parts {unexpected:?} for str {content_line}")
                        }
                    };

                    pgpass_entries.push(PgpassEntry { metadata, content });
                }
                (Some(_), None) => bail!("Odd number of lines".to_string()),
                (None, None) => break,
                _ => unreachable!(),
            }
        }

        Ok(Self(pgpass_entries))
    }
}

/// Copy to the system clipboard the psql cmd to connect to the DB matching the supplied alias with
/// Vault credentials refreshed.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let pgpass_file_path = get_pgpass_path()?;
    let pgpass_file = PgpassFile::try_from(pgpass_file_path.as_path())?;

    let mut pgpass_entry =
        match utils::tui::select::minimal::<PgpassEntry>(pgpass_file.0).closable_prompt() {
            Ok(alias) => alias,
            Err(ClosablePromptError::Closed) => return Ok(()),
            Err(error) => return Err(error.into()),
        };

    let (metadata, content) = (&pgpass_entry.metadata, &mut pgpass_entry.content);

    println!("\nLogging into Vault @ {}", std::env::var("VAULT_ADDR")?);
    log_into_vault_if_required()?;
    let vault_read_output: VaultReadOutput = serde_json::from_slice(
        &Command::new("vault")
            .args(["read", &metadata.vault_path, "--format=json"])
            .output()?
            .stdout,
    )?;

    content.update(vault_read_output.data);
    pgpass_file.save(&pgpass_file_path)?;

    let db_url = content.db_url();
    println!("\nConnecting to {}:\n", metadata.alias);
    println!("{db_url}\n");

    if let Some(psql_exit_code) = Command::new("psql")
        .arg(&db_url)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?
        .wait()?
        .code()
    {
        std::process::exit(psql_exit_code);
    }

    eprintln!("psql {db_url} terminated by signal.");
    std::process::exit(1);
}

fn get_pgpass_path() -> color_eyre::Result<PathBuf> {
    let mut pgpass_path = PathBuf::from(std::env::var("HOME")?);
    pgpass_path.push(".pgpass");
    Ok(pgpass_path)
}

// fn read_pgpass_lines(pgpass_path: &Path) -> color_eyre::Result<Vec<(usize, String)>> {
//     Ok(std::fs::read_to_string(pgpass_path)?
//         .lines()
//         .enumerate()
//         .map(|(idx, line)| (idx, line.to_string()))
//         .collect())
// }
//
// fn get_metadata_lines(lines: &[(usize, String)]) -> Vec<&(usize, String)> {
//     lines
//         .iter()
//         .filter(|(_, line)| line.starts_with('#'))
//         .collect()
// }
//
// fn find_metadata_line<'a>(
//     lines: &'a [&(usize, String)],
//     alias: &'a str,
// ) -> color_eyre::Result<&'a (usize, String)> {
//     match lines
//         .iter()
//         .filter(|(_, line)| line.contains(&format!("#{alias} ")))
//         .collect::<Vec<_>>()
//         .as_slice()
//     {
//         &[line] => Ok(line),
//         [] => bail!("no matching metadata line for alias {alias}"),
//         _ => bail!("multiple metadata lines matching alias {alias}"),
//     }
// }
//
// fn get_pgpass_line(
//     lines: &[(usize, String)],
//     metadata_line_idx: usize,
// ) -> color_eyre::Result<PgpassLine> {
//     let pgpass_line_idx = metadata_line_idx + 1;
//     lines
//         .get(metadata_line_idx + 1)
//         .map(PgpassLine::try_from)
//         .ok_or_else(|| eyre!("no PgPassLine found at idx {pgpass_line_idx} for metadata line {metadata_line_idx}"))?
// }
//
// fn extract_vault_path(metadata_line: &str) -> color_eyre::Result<&str> {
//     match metadata_line
//         .split_whitespace()
//         .collect::<Vec<_>>()
//         .as_slice()
//     {
//         &[_, vault_path] => Ok(vault_path),
//         _ => bail!("malformed metadata line {metadata_line}"),
//     }
// }

#[derive(Debug, PartialEq, Eq)]
struct PgpassEntry {
    pub metadata: Metadata,
    pub content: Content,
}

#[derive(Debug, PartialEq, Eq)]
struct Metadata {
    pub alias: String,
    pub vault_path: String,
}

#[derive(Debug, PartialEq, Eq)]
struct Content {
    pub host: String,
    pub port: i32,
    pub db: String,
    pub user: String,
    pub pwd: String,
}

impl Content {
    pub fn db_url(&self) -> String {
        format!(
            "postgres://{}@{}:{}/{}",
            self.user, self.host, self.port, self.db
        )
    }

    pub fn update(&mut self, creds: Credentials) {
        self.user = creds.username;
        self.pwd = creds.password;
    }
}

// impl PgpassEntry {
//     pub fn db_url(&self) -> String {
//         format!(
//             "postgres://{}@{}:{}/{}",
//             self.user, self.host, self.port, self.db
//         )
//     }
//
//     pub fn update(&mut self, creds: Credentials) {
//         self.user = creds.username;
//         self.pwd = creds.password;
//     }
// }

// impl TryFrom<&(usize, &str)> for PgpassLine {
//     type Error = eyre::Error;
//
//     fn try_from((idx, s): &(usize, &str)) -> Result<Self, Self::Error> {
//         match s.split(':').collect::<Vec<_>>().as_slice() {
//             [host, port, db, user, pwd] => Ok(Self {
//                 idx: *idx,
//                 host: host.to_string(),
//                 port: port
//                     .parse()
//                     .context(format!("unexpected port value, found {port}, required i32"))?,
//                 db: db.to_string(),
//                 user: user.to_string(),
//                 pwd: pwd.to_string(),
//             }),
//             unexpected => bail!("unexpected split parts {unexpected:?} for str {s}"),
//         }
//     }
// }

impl Display for PgpassEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.metadata.alias)
    }
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct VaultReadOutput {
    pub request_id: String,
    pub lease_id: String,
    pub lease_duration: i32,
    pub renewable: bool,
    pub data: Credentials,
    pub warnings: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct Credentials {
    pub password: String,
    pub username: String,
}

fn log_into_vault_if_required() -> color_eyre::Result<()> {
    let token_lookup = Command::new("vault").args(["token", "lookup"]).output()?;
    if token_lookup.status.success() {
        return Ok(());
    }
    let stderr = std::str::from_utf8(&token_lookup.stderr)?.trim();
    if !stderr.contains("permission denied") {
        bail!("unexpected error checking Vault token - error {stderr}")
    }

    let login = Command::new("vault")
        .args(["login", "-method=oidc", "-path=okta", "--no-print"])
        .output()?;
    if !login.status.success() {
        bail!(
            "error authenticating to Vault - error {}",
            std::str::from_utf8(&login.stderr)?.trim()
        )
    }

    Ok(())
}

// fn save_new_pgpass_file(lines: Vec<(usize, String)>, pgpass_path: &Path) -> color_eyre::Result<()> {
//     let mut tmp_path = PathBuf::from(pgpass_path);
//     tmp_path.set_file_name(".pgpass.tmp");
//     let mut tmp_file = File::create(&tmp_path)?;
//     for (_, line) in lines {
//         writeln!(tmp_file, "{}", line)?;
//     }
//
//     std::fs::rename(&tmp_path, pgpass_path)?;
//
//     let file = OpenOptions::new().read(true).open(pgpass_path)?;
//     let mut permissions = file.metadata()?.permissions();
//     permissions.set_mode(0o600);
//     file.set_permissions(permissions)?;
//
//     Ok(())
// }
//
// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn test_pgpass_line_try_from_returns_the_expected_pgpass_line() {
//         assert_eq!(
//             PgpassLine {
//                 idx: 42,
//                 host: "host".into(),
//                 port: 5432,
//                 db: "db".into(),
//                 user: "user".into(),
//                 pwd: "pwd".into(),
//             },
//             PgpassLine::try_from(&(42, "host:5432:db:user:pwd".into())).unwrap()
//         )
//     }
//
//     #[test]
//     fn test_pgpass_line_try_from_returns_an_error_if_port_is_not_a_number() {
//         let res = PgpassLine::try_from(&(42, "host:foo:db:user:pwd".into()));
//         assert!(
//             format!("{:?}", res).contains("Err(unexpected port value, found foo, required i32\n\nCaused by:\n    invalid digit found in string\n\nLocation:\n    src/vpg/src/main.rs:")
//         )
//     }
//
//     #[test]
//     fn test_pgpass_line_try_from_returns_an_error_if_str_is_malformed() {
//         let res = PgpassLine::try_from(&(42, "host:5432:db:user".into()));
//         assert!(
//             format!( "{:?}", res).contains("Err(unexpected split parts [\"host\", \"5432\", \"db\", \"user\"] for str host:5432:db:user\n\nLocation:\n    src/vpg/src/main.rs:")
//         )
//     }
//
//     #[test]
//     fn test_pgpass_line_db_url_returns_the_expected_output() {
//         assert_eq!(
//             "postgres://user@host:5432/db".to_string(),
//             PgpassLine {
//                 idx: 42,
//                 host: "host".into(),
//                 port: 5432,
//                 db: "db".into(),
//                 user: "user".into(),
//                 pwd: "whatever".into()
//             }
//             .db_url()
//         )
//     }
// }
