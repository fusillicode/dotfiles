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

/// Copy to the system clipboard the pgcli cmd to connect to the DB matching the selected alias with
/// refreshed Vault credentials.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut pgpass_path = PathBuf::from(std::env::var("HOME")?);
    pgpass_path.push(".pgpass");
    let pgpass_content = std::fs::read_to_string(&pgpass_path)?;
    let pgpass_file = PgpassFile::parse(pgpass_content.as_str())?;

    let args = utils::system::get_args();
    let Some(PgpassEntry { metadata, mut conn }) =
        utils::tui::select::get_opt_from_cli_args_or_tui_select(
            &args,
            |(idx, _)| *idx == 0,
            pgpass_file.entries,
            |alias: &str| {
                Box::new(move |pgpass_entry: &PgpassEntry<'_>| pgpass_entry.metadata.alias == alias)
            },
        )?
    else {
        return Ok(());
    };

    println!(
        "\nLogging into Vault @ {}\n(be sure to have the VPN on!)",
        std::env::var("VAULT_ADDR")?
    );
    log_into_vault_if_required()?;
    let vault_read_output: VaultReadOutput = serde_json::from_slice(
        &Command::new("vault")
            .args(["read", metadata.vault_path, "--format=json"])
            .output()?
            .stdout,
    )?;

    conn.update(&vault_read_output.data);
    save_new_pgpass_file(pgpass_file.idx_lines, &conn, &pgpass_path)?;

    let db_url = conn.db_url();
    println!("\nConnecting to {} @\n\n{db_url}\n", metadata.alias);

    if let Some(psql_exit_code) = Command::new("pgcli")
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

    eprintln!("pgcli {db_url} terminated by signal.");
    std::process::exit(1);
}

/// A parsed `.pgpass` file with line references and connection entries.
///
/// Stores both raw lines (preserving comments and formatting) and validated connection entries.
/// Follows PostgreSQL's password file format: `host:port:db:user:pwd` with colon-separated fields.
#[derive(Debug)]
struct PgpassFile<'a> {
    /// Original file lines with their 0-based indices, preserving comments and metadata.
    pub idx_lines: Vec<(usize, &'a str)>,
    /// Validated connection entries parsed from non-comment lines.
    pub entries: Vec<PgpassEntry<'a>>,
}

impl<'a> PgpassFile<'a> {
    pub fn parse(pgpass_content: &'a str) -> color_eyre::eyre::Result<Self> {
        let mut idx_lines = vec![];
        let mut entries = vec![];

        let mut file_lines = pgpass_content.lines().enumerate();
        while let Some(idx_line @ (_, line)) = file_lines.next() {
            idx_lines.push(idx_line);

            if line.is_empty() {
                continue;
            }

            if let Some((alias, vault_path)) =
                line.strip_prefix('#').and_then(|s| s.split_once(' '))
            {
                let metadata = Metadata { alias, vault_path };

                if let Some(idx_line) = file_lines.next() {
                    idx_lines.push(idx_line);

                    let conn = Conn::try_from(idx_line)?;
                    entries.push(PgpassEntry { metadata, conn });

                    continue;
                }
                bail!("missing expected conn line after metadata line {metadata:?} obtained from idx_line {idx_line:?}")
            }
        }

        Ok(Self { idx_lines, entries })
    }
}

/// A validated `.pgpass` entry with associated metadata and connection parameters.
#[derive(Debug, Clone)]
struct PgpassEntry<'a> {
    /// Metadata from preceding comment lines (alias/vault references).
    pub metadata: Metadata<'a>,
    /// Parsed connection parameters from a valid `.pgpass` line.
    pub conn: Conn<'a>,
}

impl<'a> std::fmt::Display for PgpassEntry<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.metadata.alias)
    }
}

/// Metadata extracted from comment lines preceding a `.pgpass` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Metadata<'a> {
    /// Human-readable identifier for the connection (from comments).
    pub alias: &'a str,
    /// Vault path reference for secure password management (from comments).
    pub vault_path: &'a str,
}

impl<'a> std::fmt::Display for Metadata<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.alias)
    }
}

/// Connection parameters parsed from a `.pgpass` line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Conn<'a> {
    /// 0-based index referencing the original line in `PgpassFile.idx_lines`.
    pub idx: usize,
    /// Hostname.
    pub host: &'a str,
    /// TCP port number.
    pub port: u16,
    /// Database name.
    pub db: &'a str,
    /// Username.
    pub user: &'a str,
    /// Password.
    pub pwd: &'a str,
}

impl<'a> Conn<'a> {
    pub fn db_url(&self) -> String {
        format!(
            "postgres://{}@{}:{}/{}",
            self.user, self.host, self.port, self.db
        )
    }

    pub fn update(&mut self, conn: &'a VaultCreds) {
        self.user = conn.username.as_str();
        self.pwd = conn.password.as_str();
    }
}

impl<'a> TryFrom<(usize, &'a str)> for Conn<'a> {
    type Error = color_eyre::eyre::Error;

    fn try_from(idx_line @ (idx, line): (usize, &'a str)) -> Result<Self, Self::Error> {
        if let [host, port, db, user, pwd] = line.split(':').collect::<Vec<_>>().as_slice() {
            let port = port
                .parse()
                .context(format!("unexpected port value {port}"))?;
            return Ok(Conn {
                idx,
                host,
                port,
                db,
                user,
                pwd,
            });
        }
        bail!("cannot build CredsLine from idx_line {idx_line:?}")
    }
}

impl<'a> std::fmt::Display for Conn<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}:{}",
            self.host, self.port, self.db, self.user, self.pwd
        )
    }
}

/// Response structure from Vault's secret read operations.
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct VaultReadOutput {
    /// Unique request identifier for tracing.
    pub request_id: String,
    /// Lease identifier for secret lifecycle management.
    pub lease_id: String,
    /// Time-to-live duration in seconds for secret.
    pub lease_duration: i32,
    /// Indicates if lease can be renewed.
    pub renewable: bool,
    /// Contains actual secret credentials.
    pub data: VaultCreds,
    /// Non-critical operational warnings.
    pub warnings: Vec<String>,
}

/// Database credentials stored in Vault.
#[derive(Deserialize, Debug)]
struct VaultCreds {
    /// Database password.
    pub password: String,
    /// Database username.
    pub username: String,
}

/// Checks and renews Vault authentication using OIDC/Okta if token is invalid.
///
/// # Workflow
/// 1. Checks current Vault token validity via `vault token lookup`.
/// 2. If valid, returns immediately.
/// 3. If token is invalid due to permission error, initiates OIDC login via Okta.
/// 4. Fails on unexpected lookup errors or login failures.
///
/// # Errors
/// Returns errors for non-permission-denied lookup failures or failed logins.
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

/// Saves updated PostgreSQL .pgpass credentials to a temporary file, replaces the original, and sets permissions.
///
/// # Arguments
/// * `pgpass_idx_lines` - Original file lines with their indices (to identify line needing update).
/// * `updated_creds` - New connection credentials (must implement `ToString`).
/// * `pgpass_path` - Path to the original .pgpass file.
///
/// # Workflow
/// 1. Creates temporary file `.pgpass.tmp` in same directory.
/// 2. Writes all lines, replacing the specified index with updated credentials.
/// 3. Atomically replaces original file via rename.
/// 4. Sets strict permissions (600) to match .pgpass security requirements.
fn save_new_pgpass_file(
    pgpass_idx_lines: Vec<(usize, &str)>,
    updated_creds: &Conn,
    pgpass_path: &Path,
) -> color_eyre::Result<()> {
    let mut tmp_path = PathBuf::from(pgpass_path);
    tmp_path.set_file_name(".pgpass.tmp");
    let mut tmp_file = File::create(&tmp_path)?;

    for (idx, pgpass_line) in pgpass_idx_lines {
        let file_line_content = if idx == updated_creds.idx {
            updated_creds.to_string()
        } else {
            pgpass_line.to_string()
        };
        writeln!(tmp_file, "{file_line_content}")?;
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
    fn test_creds_try_from_returns_the_expected_creds() {
        assert_eq!(
            Conn {
                idx: 42,
                host: "host",
                port: 5432,
                db: "db",
                user: "user",
                pwd: "pwd",
            },
            Conn::try_from((42, "host:5432:db:user:pwd")).unwrap()
        )
    }

    #[test]
    fn test_creds_try_from_returns_an_error_if_port_is_not_a_number() {
        let res = Conn::try_from((42, "host:foo:db:user:pwd"));
        assert!(
            format!("{res:?}").contains("Err(unexpected port value foo\n\nCaused by:\n    invalid digit found in string\n\nLocation:\n    src/vpg/src/main.rs:")
        )
    }

    #[test]
    fn test_creds_try_from_returns_an_error_if_str_is_malformed() {
        let res = Conn::try_from((42, "host:5432:db:user"));
        assert!(format!("{res:?}")
            .contains("Err(cannot build CredsLine from idx_line (42, \"host:5432:db:user\")\n\nLocation:\n    src/vpg/src/main.rs:"))
    }

    #[test]
    fn test_creds_db_url_returns_the_expected_output() {
        assert_eq!(
            "postgres://user@host:5432/db".to_string(),
            Conn {
                idx: 42,
                host: "host",
                port: 5432,
                db: "db",
                user: "user",
                pwd: "whatever"
            }
            .db_url()
        )
    }
}
