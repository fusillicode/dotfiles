use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;

use crate::agent::Agent;
use crate::agent::session::Session;
use crate::agent::session::SessionKey;

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

/// Load only the requested resumable sessions from their owning agent stores.
///
/// # Errors
/// Returns an error when a matching supported session cannot be read or parsed.
pub fn load_sessions_by_key(keys: &[SessionKey]) -> rootcause::Result<Vec<Session>> {
    let mut sessions = Vec::new();
    sessions.extend(claude::load_sessions_by_key(keys)?);
    sessions.extend(codex::load_sessions_by_key(keys)?);
    sessions.extend(cursor::load_sessions_by_key(keys)?);
    Ok(sessions)
}

fn requested_ids(keys: &[SessionKey], agent: Agent) -> HashSet<&str> {
    keys.iter()
        .filter(|key| key.agent() == agent)
        .map(SessionKey::id)
        .collect()
}

fn find_session_paths(
    root: &Path,
    matching_file_fn: impl Fn(&std::fs::DirEntry) -> bool,
    skip_dir_fn: impl Fn(&std::fs::DirEntry) -> bool,
) -> rootcause::Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    ytil_sys::file::find_matching_recursively_in_dir(root, matching_file_fn, skip_dir_fn)
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

#[cfg(test)]
mod tests {
    use test_that::prelude::*;
    #[test]
    fn test_find_session_paths_missing_root_returns_empty_paths() {
        let dir = tempfile::tempdir().unwrap();
        let missing_root = dir.path().join("missing");

        let res = crate::agent::session_loader::find_session_paths(&missing_root, |_| true, |_| false);

        assert_that!(res, ok(eq(Vec::<std::path::PathBuf>::new())));
    }

    #[test]
    fn test_find_session_paths_existing_file_root_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let file_root = dir.path().join("file");
        std::fs::write(&file_root, b"not a directory").unwrap();

        let res = crate::agent::session_loader::find_session_paths(&file_root, |_| true, |_| false);

        assert_that!(
            (res).map(|_| ()),
            err(displays_as(contains_substring("error reading directory")))
        );
    }
}
