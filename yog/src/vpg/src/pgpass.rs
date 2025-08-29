use std::borrow::Cow;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use color_eyre::eyre::WrapErr;
use color_eyre::eyre::bail;
use color_eyre::owo_colors::OwoColorize;
use utils::sk::SkimItem;
use utils::sk::SkimItemPreview;
use utils::sk::SkimPreviewContext;

use crate::vault::VaultCreds;

/// A parsed `.pgpass` file with line references and connection entries.
///
/// Stores both raw lines (preserving comments and formatting) and validated connection entries.
/// Follows PostgreSQL's password file format: `host:port:db:user:pwd` with colon-separated fields.
#[derive(Debug)]
pub struct PgpassFile<'a> {
    /// Original file lines with their 0-based indices, preserving comments and metadata.
    pub idx_lines: Vec<(usize, &'a str)>,
    /// Validated connection entries parsed from non-comment lines.
    pub entries: Vec<PgpassEntry>,
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

            if let Some((alias, vault_path)) = line.strip_prefix('#').and_then(|s| s.split_once(' ')) {
                let metadata = Metadata {
                    alias: alias.to_string(),
                    vault_path: vault_path.to_string(),
                };

                if let Some(idx_line) = file_lines.next() {
                    idx_lines.push(idx_line);

                    let conn = ConnectionParams::try_from(idx_line)?;
                    entries.push(PgpassEntry {
                        metadata,
                        connection_params: conn,
                    });

                    continue;
                }
                bail!(
                    "missing expected conn line after metadata line {metadata:#?} obtained from idx_line {idx_line:#?}"
                )
            }
        }

        Ok(Self { idx_lines, entries })
    }
}

/// A validated `.pgpass` entry with associated metadata and connection parameters.
#[derive(Debug, Clone)]
pub struct PgpassEntry {
    /// Parsed connection parameters from a valid `.pgpass` line.
    pub connection_params: ConnectionParams,
    /// Metadata from preceding comment lines (alias/vault references).
    pub metadata: Metadata,
}

impl SkimItem for PgpassEntry {
    fn text(&self) -> Cow<'_, str> {
        Cow::from(self.to_string())
    }

    fn preview(&self, _context: SkimPreviewContext) -> SkimItemPreview {
        SkimItemPreview::AnsiText(format!(
            "Host: {}\nUser: {}\nDb: {}\nPort: {}\nVault: {}\n",
            self.connection_params.host.bold(),
            self.connection_params.user.bold(),
            self.connection_params.db.bold(),
            self.connection_params.port.bold(),
            self.metadata.vault_path.bold(),
        ))
    }
}

impl core::fmt::Display for PgpassEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.metadata.alias)
    }
}

/// Metadata extracted from comment lines preceding a `.pgpass` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// Human-readable identifier for the connection (from comments).
    pub alias: String,
    /// Vault path reference for secure password management (from comments).
    pub vault_path: String,
}

impl core::fmt::Display for Metadata {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.alias)
    }
}

/// Connection parameters parsed from a `.pgpass` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionParams {
    /// Database name.
    db: String,
    /// Hostname.
    host: String,
    /// 0-based index referencing the original line in `PgpassFile.idx_lines`.
    idx: usize,
    /// TCP port number.
    port: u16,
    /// Password.
    pwd: String,
    /// Username.
    user: String,
}

impl ConnectionParams {
    /// Generates a PostgreSQL connection [`String`] URL from the connection parameters.
    pub fn db_url(&self) -> String {
        format!("postgres://{}@{}:{}/{}", self.user, self.host, self.port, self.db)
    }

    /// Updates the user and password fields with the provided [`VaultCreds`].
    pub fn update(&mut self, creds: &VaultCreds) {
        self.user = creds.username.to_string();
        self.pwd = creds.password.to_string();
    }
}

impl TryFrom<(usize, &str)> for ConnectionParams {
    type Error = color_eyre::eyre::Error;

    fn try_from(idx_line @ (idx, line): (usize, &str)) -> Result<Self, Self::Error> {
        if let [host, port, db, user, pwd] = line.split(':').collect::<Vec<_>>().as_slice() {
            let port = port.parse().context(format!("unexpected port value {port}"))?;
            return Ok(ConnectionParams {
                idx,
                host: host.to_string(),
                port,
                db: db.to_string(),
                user: user.to_string(),
                pwd: pwd.to_string(),
            });
        }
        bail!("cannot build CredsLine from idx_line {idx_line:#?}")
    }
}

impl core::fmt::Display for ConnectionParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}:{}:{}:{}", self.host, self.port, self.db, self.user, self.pwd)
    }
}

/// Saves updated PostgreSQL .pgpass to a temporary file, replaces the original, and sets permissions.
///
/// # Arguments
/// * `pgpass_idx_lines` - Original file lines with their indices (to identify line needing update).
/// * `updated_conn_params` - New connection parameters (must implement `ToString`).
/// * `pgpass_path` - Path to the original .pgpass file.
///
/// # Workflow
/// 1. Creates temporary file `.pgpass.tmp` in same directory.
/// 2. Writes all lines, replacing the specified index with updated connection parameters.
/// 3. Atomically replaces original file via rename.
/// 4. Sets strict permissions (600) to match .pgpass security requirements.
pub fn save_new_pgpass_file(
    pgpass_idx_lines: Vec<(usize, &str)>,
    updated_conn_params: &ConnectionParams,
    pgpass_path: &Path,
) -> color_eyre::Result<()> {
    let mut tmp_path = PathBuf::from(pgpass_path);
    tmp_path.set_file_name(".pgpass.tmp");
    let mut tmp_file = File::create(&tmp_path)?;

    for (idx, pgpass_line) in pgpass_idx_lines {
        let file_line = if idx == updated_conn_params.idx {
            updated_conn_params.to_string()
        } else {
            pgpass_line.to_string()
        };
        writeln!(tmp_file, "{file_line}")?;
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
    fn creds_try_from_returns_the_expected_creds() {
        assert_eq!(
            ConnectionParams {
                idx: 42,
                host: "host".into(),
                port: 5432,
                db: "db".into(),
                user: "user".into(),
                pwd: "pwd".into(),
            },
            ConnectionParams::try_from((42, "host:5432:db:user:pwd")).unwrap()
        )
    }

    #[test]
    fn creds_try_from_returns_an_error_if_port_is_not_a_number() {
        let res = format!("{:#?}", ConnectionParams::try_from((42, "host:foo:db:user:pwd")));
        assert!(
            res.contains("Err(\n    Error {\n        msg: \"unexpected port value foo\",\n        source: ParseIntError {\n            kind: InvalidDigit,\n        },\n    },\n)"),
            "unexpected {res}"
        )
    }

    #[test]
    fn creds_try_from_returns_an_error_if_str_is_malformed() {
        let res = format!("{:#?}", ConnectionParams::try_from((42, "host:5432:db:user")));
        assert!(
            res.contains("Err(\n    \"cannot build CredsLine from idx_line (\\n    42,\\n    \\\"host:5432:db:user\\\",\\n)\",\n)"),
            "unexpected {res}"
        )
    }

    #[test]
    fn creds_db_url_returns_the_expected_output() {
        assert_eq!(
            "postgres://user@host:5432/db".to_string(),
            ConnectionParams {
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
