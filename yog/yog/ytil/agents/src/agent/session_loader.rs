use std::path::Path;

use rootcause::prelude::ResultExt as _;

use crate::agent::session::Session;

pub mod claude;
pub mod codex;
pub mod cursor;

/// Load resumable sessions from every supported local agent store.
///
/// # Errors
/// Returns an error when any supported session store cannot be read or parsed.
pub fn load_sessions() -> rootcause::Result<Vec<Session>> {
    let mut sessions = Vec::new();
    sessions.extend(claude::load_sessions()?);
    sessions.extend(codex::load_sessions()?);
    sessions.extend(cursor::load_sessions()?);
    Ok(sessions)
}

fn file_updated_at(path: &Path) -> rootcause::Result<Option<chrono::DateTime<chrono::Utc>>> {
    let modified = std::fs::metadata(path)
        .context("failed to read session metadata")
        .attach_with(|| format!("path={}", path.display()))?
        .modified()
        .context("failed to read session modified time")
        .attach_with(|| format!("path={}", path.display()))?;
    Ok(Some(chrono::DateTime::<chrono::Utc>::from(modified)))
}
