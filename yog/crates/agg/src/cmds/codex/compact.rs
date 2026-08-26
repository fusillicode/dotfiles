use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use rusqlite::Connection;
use rusqlite::OpenFlags;

const DATABASE_FILE_NAME: &str = "logs_2.sqlite";
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(3);

/// Compact the local Codex log database after validating its integrity and WAL checkpoint state.
///
/// # Errors
/// - The configured Codex database does not exist or is not a regular file.
/// - SQLite cannot obtain a write lock within three seconds.
/// - An integrity check fails, WAL checkpoint is busy or incomplete, or vacuuming fails.
pub fn run() -> rootcause::Result<()> {
    let database_path = database_path()?;
    let connection = open_database(&database_path)?;

    verify_integrity(&connection, "pre-vacuum")?;

    println!("{}", "Checkpointing the Codex log database".blue().bold());
    let checkpoint = checkpoint_wal(&connection)?;
    println!(
        "{} busy={} log_frames={} checkpointed_frames={}",
        "Checkpoint complete:".green().bold(),
        checkpoint.busy,
        checkpoint.log_frames,
        checkpoint.checkpointed_frames
    );

    println!("{}", "Compacting the active Codex log database".blue().bold());
    connection.execute_batch("VACUUM;")?;

    verify_integrity(&connection, "post-vacuum")?;
    let stats = database_stats(&connection)?;
    println!(
        "{} page_count={} freelist_count={}",
        "Compaction complete:".green().bold(),
        stats.page_count,
        stats.freelist_count
    );

    Ok(())
}

fn database_path() -> rootcause::Result<PathBuf> {
    let codex_home = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map_or_else(
            || ytil_sys::dir::build_home_path(&[".codex"]),
            |value| Ok(PathBuf::from(value)),
        )?;
    let database_path = codex_home.join(DATABASE_FILE_NAME);
    let metadata = database_path.metadata().map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            report!("Codex log database not found").attach(format!("path={}", database_path.display()))
        } else {
            report!("cannot read Codex log database metadata")
                .attach(format!("path={} error={error}", database_path.display()))
        }
    })?;

    if !metadata.is_file() {
        return Err(
            report!("Codex log database is not a regular file").attach(format!("path={}", database_path.display()))
        );
    }

    Ok(database_path)
}

fn open_database(database_path: &Path) -> rootcause::Result<Connection> {
    let connection = Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .attach_with(|| format!("cannot open Codex log database | path={}", database_path.display()))?;
    connection.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
    Ok(connection)
}

fn verify_integrity(connection: &Connection, phase: &str) -> rootcause::Result<()> {
    let mut statement = connection.prepare("PRAGMA quick_check;")?;
    let results = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    if results.as_slice() != ["ok"] {
        return Err(
            report!("Codex log database integrity check failed").attach(format!("phase={phase} results={results:#?}"))
        );
    }

    Ok(())
}

fn checkpoint_wal(connection: &Connection) -> rootcause::Result<CheckpointState> {
    let checkpoint = connection.query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |row| {
        Ok(CheckpointState {
            busy: row.get(0)?,
            log_frames: row.get(1)?,
            checkpointed_frames: row.get(2)?,
        })
    })?;

    if checkpoint.busy != 0 {
        return Err(report!("Codex log database WAL checkpoint is busy").attach(format!("checkpoint={checkpoint:#?}")));
    }
    if checkpoint.log_frames != checkpoint.checkpointed_frames {
        return Err(report!("Codex log database WAL truncate checkpoint is incomplete")
            .attach(format!("checkpoint={checkpoint:#?}")));
    }

    Ok(checkpoint)
}

fn database_stats(connection: &Connection) -> rootcause::Result<DatabaseStats> {
    connection
        .query_row(
            "SELECT page_count, freelist_count FROM pragma_page_count(), pragma_freelist_count();",
            [],
            |row| {
                Ok(DatabaseStats {
                    page_count: row.get(0)?,
                    freelist_count: row.get(1)?,
                })
            },
        )
        .map_err(Into::into)
}

#[derive(Debug)]
struct CheckpointState {
    busy: i64,
    log_frames: i64,
    checkpointed_frames: i64,
}

struct DatabaseStats {
    page_count: i64,
    freelist_count: i64,
}
