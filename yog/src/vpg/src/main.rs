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

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use serde::Deserialize;

/// Copy to the system clipboard the psql cmd to connect to the DB matching the supplied alias with
/// Vault credentials refreshed.
fn main() -> anyhow::Result<()> {
    std::env::var("VAULT_ADDR").context("VAULT_ADDR missing")?;

    let args = utils::system::get_args();
    let Some(alias) = args.first() else {
        bail!("no alias specified {:?}", args);
    };
    let pgpass_path = get_pgpass_path()?;
    let mut lines = read_pgpass_lines(&pgpass_path)?;

    // We do all this before interacting with Vault to avoid unneeded calls.
    let (metadata_line_idx, metadata_line) = find_metadata_line(&lines, alias)?;
    let mut pgpass_line = get_pgpass_line(&lines, *metadata_line_idx)?;
    let vault_path = extract_vault_path(metadata_line)?;

    login_to_vault_if_required()?;

    let vault_read_output: VaultReadOutput = serde_json::from_slice(
        &Command::new("vault")
            .args(["read", vault_path, "--format=json"])
            .output()?
            .stdout,
    )?;

    pgpass_line.update(vault_read_output.data, &mut lines);

    save_new_pgpass_file(lines, &pgpass_path)?;

    let db_url = pgpass_line.db_url();
    println!("\nConnecting to {alias}:\n");
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

fn get_pgpass_path() -> anyhow::Result<PathBuf> {
    let mut pgpass_path = PathBuf::from(std::env::var("HOME")?);
    pgpass_path.push(".pgpass");
    Ok(pgpass_path)
}

fn read_pgpass_lines(pgpass_path: &Path) -> anyhow::Result<Vec<(usize, String)>> {
    Ok(std::fs::read_to_string(pgpass_path)?
        .lines()
        .enumerate()
        .map(|(idx, line)| (idx, line.to_string()))
        .collect())
}

fn find_metadata_line<'a>(
    lines: &'a [(usize, String)],
    alias: &'a str,
) -> anyhow::Result<&'a (usize, String)> {
    match lines
        .iter()
        .filter(|(_, line)| line.contains(&format!("#{alias} ")))
        .collect::<Vec<_>>()
        .as_slice()
    {
        &[line] => Ok(line),
        [] => bail!("no matching metadata line for alias {alias}"),
        _ => bail!("multiple lines matching alias {alias}"),
    }
}

fn get_pgpass_line(
    lines: &[(usize, String)],
    metadata_line_idx: usize,
) -> anyhow::Result<PgpassLine> {
    let pgpass_line_idx = metadata_line_idx + 1;
    lines
        .get(metadata_line_idx + 1)
        .map(PgpassLine::try_from)
        .ok_or_else(|| anyhow!("no PgPassLine found at idx {pgpass_line_idx} for metadata line {metadata_line_idx}"))?
}

fn extract_vault_path(metadata_line: &str) -> anyhow::Result<&str> {
    match metadata_line
        .split_whitespace()
        .collect::<Vec<_>>()
        .as_slice()
    {
        &[_, vault_path] => Ok(vault_path),
        _ => bail!("malformed metadata line {metadata_line}"),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct PgpassLine {
    pub idx: usize,
    pub host: String,
    pub port: i32,
    pub db: String,
    pub user: String,
    pub pwd: String,
}

impl PgpassLine {
    pub fn db_url(&self) -> String {
        format!(
            "postgres://{}@{}:{}/{}",
            self.user, self.host, self.port, self.db
        )
    }

    pub fn update(&mut self, creds: Credentials, lines: &mut [(usize, String)]) {
        self.user = creds.username;
        self.pwd = creds.password;
        lines[self.idx] = (self.idx, self.to_string());
    }
}

impl TryFrom<&(usize, String)> for PgpassLine {
    type Error = anyhow::Error;

    fn try_from((idx, s): &(usize, String)) -> Result<Self, Self::Error> {
        match s.split(':').collect::<Vec<_>>().as_slice() {
            [host, port, db, user, pwd] => Ok(Self {
                idx: *idx,
                host: host.to_string(),
                port: port
                    .parse()
                    .context(format!("unexpected port value, found {port}, required i32"))?,
                db: db.to_string(),
                user: user.to_string(),
                pwd: pwd.to_string(),
            }),
            unexpected => bail!("unexpected split parts {unexpected:?} for str {s}"),
        }
    }
}

impl Display for PgpassLine {
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

fn login_to_vault_if_required() -> anyhow::Result<()> {
    let token_lookup = Command::new("vault").args(["token", "lookup"]).output()?;
    if token_lookup.status.success() {
        return Ok(());
    }
    let stderr = std::str::from_utf8(&token_lookup.stderr)?.trim();
    if !stderr.contains("permission denied") {
        bail!("unexpected error checking Vault token, error {}", stderr)
    }

    let login = Command::new("vault")
        .args(["login", "-method=oidc", "-path=okta", "--no-print"])
        .output()?;
    if !login.status.success() {
        bail!(
            "error authenticating to Vault, error {}",
            std::str::from_utf8(&login.stderr)?.trim()
        )
    }

    Ok(())
}

fn save_new_pgpass_file(lines: Vec<(usize, String)>, pgpass_path: &Path) -> anyhow::Result<()> {
    let mut tmp_path = PathBuf::from(pgpass_path);
    tmp_path.set_file_name(".pgpass.tmp");
    let mut tmp_file = File::create(&tmp_path)?;
    for (_, line) in lines {
        writeln!(tmp_file, "{}", line)?;
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
    fn test_pgpass_line_try_from_returns_the_expected_pgpass_line() {
        assert_eq!(
            PgpassLine {
                idx: 42,
                host: "host".into(),
                port: 5432,
                db: "db".into(),
                user: "user".into(),
                pwd: "pwd".into(),
            },
            PgpassLine::try_from(&(42, "host:5432:db:user:pwd".into())).unwrap()
        )
    }

    #[test]
    fn test_pgpass_line_try_from_returns_an_error_if_port_is_not_a_number() {
        assert_eq!(
            "Err(unexpected port value, found foo, required i32\n\nCaused by:\n    invalid digit found in string)",
            format!("{:?}", PgpassLine::try_from(&(42, "host:foo:db:user:pwd".into())))
        )
    }

    #[test]
    fn test_pgpass_line_try_from_returns_an_error_if_str_is_malformed() {
        assert_eq!(
            r#"Err(unexpected split parts ["host", "5432", "db", "user"] for str host:5432:db:user)"#,
            format!(
                "{:?}",
                PgpassLine::try_from(&(42, "host:5432:db:user".into()))
            )
        )
    }

    #[test]
    fn test_pgpass_line_db_url_returns_the_expected_output() {
        assert_eq!(
            "postgres://user@host:5432/db".to_string(),
            PgpassLine {
                idx: 42,
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
