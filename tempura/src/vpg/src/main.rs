#![feature(exit_status_error)]

use std::fmt::Display;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use serde::Deserialize;

/// Connect to Postgres DB via alias & Vault.
fn main() -> anyhow::Result<()> {
    std::env::var("VAULT_ADDR")?;

    let args = utils::system::get_args();
    let Some(alias) = args.first() else {
        bail!("no alias specified {:?}", args);
    };

    let mut pgpass_path = PathBuf::from(std::env::var("HOME")?);
    pgpass_path.push(".pgpass");

    let pgpass_content = std::fs::read_to_string(&pgpass_path)?;
    let mut lines = pgpass_content
        .lines()
        .enumerate()
        .collect::<Vec<(usize, &str)>>();

    let (metadata_line_idx, metadata_line) = match lines
        .iter()
        .filter(|(_, line)| line.contains(&format!("#{alias} ")))
        .collect::<Vec<_>>()
        .as_slice()
    {
        &[line] => line,
        [] => bail!("no matching metadata line for alias {alias} in file {pgpass_path:?}"),
        _ => bail!("multiple lines matching alias {alias} in file {pgpass_path:?}"),
    };

    let pgpass_line_idx = metadata_line_idx + 1;
    let mut pgpass_line = lines
        .get(metadata_line_idx + 1)
        .map(|(_, line)| PgpassLine::from_str(*line))
        .ok_or_else(|| anyhow!("no PgPassLine found at idx {pgpass_line_idx} for metadata line {metadata_line_idx} {metadata_line}"))??;

    let &[_, vault_path] = metadata_line
        .split_whitespace()
        .collect::<Vec<_>>()
        .as_slice()
    else {
        bail!(
            "malformed metadata line {metadata_line_idx} {metadata_line} in file {pgpass_path:?}"
        );
    };

    login_to_vault_if_required()?;

    let vault_read_output: VaultPathOutput = serde_json::from_slice(
        &Command::new("vault")
            .args(["read", vault_path, "--format=json"])
            .output()?
            .stdout,
    )?;

    pgpass_line.user = vault_read_output.data.username;
    pgpass_line.pwd = vault_read_output.data.password;
    let new_pgpass_line = pgpass_line.to_string();
    lines[pgpass_line_idx] = (pgpass_line_idx, &new_pgpass_line);

    save_new_pgpass_file(lines, &pgpass_path)?;

    Ok(())
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

#[derive(Debug, PartialEq, Eq)]
struct PgpassLine {
    pub host: String,
    pub port: i32,
    pub db: String,
    pub user: String,
    pub pwd: String,
}

impl FromStr for PgpassLine {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split(':').collect::<Vec<_>>().as_slice() {
            [host, port, db, user, pwd] => Ok(Self {
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
struct VaultPathOutput {
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

fn save_new_pgpass_file(lines: Vec<(usize, &str)>, pgpass_path: &Path) -> anyhow::Result<()> {
    let mut tmp_path = PathBuf::from(pgpass_path);
    tmp_path.set_file_name(".pgpass.tmp");
    let mut tmp_file = File::create(&tmp_path)?;
    for (_, line) in lines {
        writeln!(tmp_file, "{}", line)?;
    }
    std::fs::rename(&tmp_path, pgpass_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pgpass_line_from_str_returns_the_expected_pgpass_line() {
        assert_eq!(
            PgpassLine {
                host: "host".into(),
                port: 5432,
                db: "db".into(),
                user: "user".into(),
                pwd: "pwd".into(),
            },
            PgpassLine::from_str("host:5432:db:user:pwd").unwrap()
        )
    }

    #[test]
    fn test_pgpass_line_from_str_returns_an_error_if_port_is_not_a_number() {
        assert_eq!(
            "Err(unexpected port value, found foo, required i32\n\nCaused by:\n    invalid digit found in string)",
            format!("{:?}", PgpassLine::from_str("host:foo:db:user:pwd"))
        )
    }

    #[test]
    fn test_pgpass_line_from_str_returns_an_error_if_str_is_malformed() {
        assert_eq!(
            r#"Err(unexpected split parts ["host", "5432", "db", "user"] for str host:5432:db:user)"#,
            format!("{:?}", PgpassLine::from_str("host:5432:db:user"))
        )
    }
}
