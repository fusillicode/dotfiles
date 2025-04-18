#![feature(exit_status_error)]

use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use color_eyre::eyre::bail;
use color_eyre::eyre::WrapErr;
use serde::Deserialize;

use utils::tui::ClosablePrompt;
use utils::tui::ClosablePromptError;

/// Copy to the system clipboard the psql cmd to connect to the DB matching the supplied alias with
/// Vault credentials refreshed.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut pgpass_path = PathBuf::from(std::env::var("HOME")?);
    pgpass_path.push(".pgpass");
    let pgpass_content = std::fs::read_to_string(&pgpass_path)?;
    let PgpassFile {
        file_lines,
        pgpass_entries,
    } = PgpassFile::try_from(pgpass_content.as_str())?;

    let PgpassEntry {
        metadata_line,
        mut content_line,
    } = match utils::tui::select::minimal::<PgpassEntry>(pgpass_entries).closable_prompt() {
        Ok(alias) => alias,
        Err(ClosablePromptError::Closed) => return Ok(()),
        Err(error) => return Err(error.into()),
    };

    println!("\nLogging into Vault @ {}", std::env::var("VAULT_ADDR")?);
    log_into_vault_if_required()?;
    let vault_read_output: VaultReadOutput = serde_json::from_slice(
        &Command::new("vault")
            .args(["read", &metadata_line.vault_path, "--format=json"])
            .output()?
            .stdout,
    )?;

    content_line.update(vault_read_output.data);
    save_new_pgpass_file(file_lines, &content_line, &pgpass_path).unwrap();

    let db_url = content_line.db_url();
    println!("\nConnecting to {}:\n", metadata_line.alias);
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

#[derive(Debug)]
struct PgpassFile<'a> {
    pub file_lines: Vec<(usize, &'a str)>,
    pub pgpass_entries: Vec<PgpassEntry>,
}

impl<'a> TryFrom<&'a str> for PgpassFile<'a> {
    type Error = color_eyre::eyre::Error;

    fn try_from(pgpass_content: &'a str) -> Result<Self, Self::Error> {
        let mut lines: Vec<(_, &str)> = vec![];
        let mut entries = vec![];

        let mut file_lines = pgpass_content.lines().enumerate();
        while let Some((idx, line)) = file_lines.next() {
            lines.push((idx, line));

            if line.is_empty() {
                continue;
            }

            if let Some((alias, vault_path)) =
                line.strip_prefix('#').and_then(|s| s.split_once(' '))
            {
                let metadata_line = MetadataLine {
                    alias: alias.to_string(),
                    vault_path: vault_path.to_string(),
                };
                if let Some(file_line) = file_lines.next() {
                    lines.push(file_line);
                    let content_line = ContentLine::try_from(file_line)?;
                    entries.push(PgpassEntry {
                        metadata_line,
                        content_line,
                    });
                    continue;
                }
                bail!("missing expected content line after metadata line {idx} {metadata_line}")
            }
        }

        Ok(PgpassFile {
            file_lines: lines,
            pgpass_entries: entries,
        })
    }
}

#[derive(Debug)]
struct PgpassEntry {
    pub metadata_line: MetadataLine,
    pub content_line: ContentLine,
}

impl std::fmt::Display for PgpassEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.metadata_line.alias)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct MetadataLine {
    pub alias: String,
    pub vault_path: String,
}

impl std::fmt::Display for MetadataLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.alias)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ContentLine {
    pub file_line_idx: usize,
    pub host: String,
    pub port: i32,
    pub db: String,
    pub user: String,
    pub pwd: String,
}

impl ContentLine {
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

impl TryFrom<(usize, &str)> for ContentLine {
    type Error = color_eyre::eyre::Error;

    fn try_from((idx, line): (usize, &str)) -> Result<Self, Self::Error> {
        if let [host, port, db, user, pwd] = line.split(':').collect::<Vec<_>>().as_slice() {
            return Ok(ContentLine {
                file_line_idx: idx,
                host: host.to_string(),
                port: port
                    .parse()
                    .context(format!("unexpected port value, found {port}, required i32"))?,
                db: db.to_string(),
                user: user.to_string(),
                pwd: pwd.to_string(),
            });
        }
        bail!("cannot build ContentLine for pgpass line {idx} {line}")
    }
}

impl std::fmt::Display for ContentLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}:{}",
            self.host, self.port, self.db, self.user, self.pwd
        )
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

fn save_new_pgpass_file(
    file_lines: Vec<(usize, &str)>,
    new_content_line: &ContentLine,
    pgpass_path: &Path,
) -> color_eyre::Result<()> {
    let mut tmp_path = PathBuf::from(pgpass_path);
    tmp_path.set_file_name(".pgpass.tmp");
    let mut tmp_file = File::create(&tmp_path)?;

    for (idx, line) in file_lines {
        let new_line = if idx == new_content_line.file_line_idx {
            new_content_line.to_string()
        } else {
            line.to_string()
        };
        writeln!(tmp_file, "{new_line}")?;
    }

    std::fs::rename(&tmp_path, pgpass_path)?;

    let file = OpenOptions::new().read(true).open(pgpass_path)?;
    let mut permissions = file.metadata()?.permissions();
    permissions.set_mode(0o600);
    file.set_permissions(permissions)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_line_try_from_returns_the_expected_content_line() {
        assert_eq!(
            ContentLine {
                file_line_idx: 42,
                host: "host".into(),
                port: 5432,
                db: "db".into(),
                user: "user".into(),
                pwd: "pwd".into(),
            },
            ContentLine::try_from((42, "host:5432:db:user:pwd")).unwrap()
        )
    }

    #[test]
    fn test_content_line_try_from_returns_an_error_if_port_is_not_a_number() {
        let res = ContentLine::try_from((42, "host:foo:db:user:pwd"));
        assert!(
            format!("{:?}", res).contains("Err(unexpected port value, found foo, required i32\n\nCaused by:\n    invalid digit found in string\n\nLocation:\n    src/vpg/src/main.rs:")
        )
    }

    #[test]
    fn test_content_line_try_from_returns_an_error_if_str_is_malformed() {
        let res = ContentLine::try_from((42, "host:5432:db:user"));
        assert!(format!("{:?}", res)
            .contains("cannot build ContentLine for pgpass line 42 host:5432:db:user"))
    }

    #[test]
    fn test_content_line_db_url_returns_the_expected_output() {
        assert_eq!(
            "postgres://user@host:5432/db".to_string(),
            ContentLine {
                file_line_idx: 42,
                host: "host".into(),
                port: 5432,
                db: "db".into(),
                user: "user".into(),
                pwd: "whatever".into()
            }
            .db_url()
        )
    }
}
