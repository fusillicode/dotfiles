use std::path::Path;

use rootcause::prelude::ResultExt as _;

use crate::agent::Agent;
use crate::agent::session::Session;

pub fn load_sessions() -> rootcause::Result<Vec<Session>> {
    let root = ytil_sys::dir::build_home_path(Agent::Codex.sessions_root_path())?;
    let session_paths = ytil_sys::file::find_matching_recursively_in_dir(
        &root,
        |entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"),
        |_| false,
    )?;

    let mut sessions = Vec::new();
    for session_path in session_paths {
        let content = std::fs::read_to_string(&session_path)
            .context("failed to read Codex session file")
            .attach_with(|| format!("path={}", session_path.display()))?;
        let session_name = session_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let mut session = crate::agent::session_parser::codex::parse(&content, session_name)
            .attach_with(|| format!("path={}", session_path.display()))?;
        if session.workspace.is_dir() {
            session.updated_at = file_updated_at(&session_path)?.unwrap_or(session.created_at);
            session.path = session_path;
            sessions.push(session);
        }
    }

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
