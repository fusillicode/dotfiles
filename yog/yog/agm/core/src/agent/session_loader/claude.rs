use rootcause::prelude::ResultExt as _;

use crate::agent::Agent;
use crate::agent::session::Session;

pub fn load_sessions() -> rootcause::Result<Vec<Session>> {
    let root = ytil_sys::dir::build_home_path(Agent::Claude.sessions_root_path())?;
    let session_paths = ytil_sys::file::find_matching_recursively_in_dir(
        &root,
        |entry| {
            let path = entry.path();
            path.extension().is_some_and(|ext| ext == "jsonl")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !matches!(name, "sessions-index.json" | "session.json"))
        },
        |_| false,
    )?;

    let mut sessions = Vec::new();
    for session_path in session_paths {
        let content = std::fs::read_to_string(&session_path)
            .context("failed to read Claude session file")
            .attach_with(|| format!("path={}", session_path.display()))?;
        let mut session = crate::agent::session_parser::claude::parse(&content)
            .attach_with(|| format!("path={}", session_path.display()))?;
        if session.workspace.is_dir() {
            session.updated_at = super::file_updated_at(&session_path)?.unwrap_or(session.created_at);
            session.path = session_path;
            sessions.push(session);
        }
    }

    Ok(sessions)
}
