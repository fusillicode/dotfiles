use rootcause::prelude::ResultExt;

use crate::agent::Agent;
use crate::agent::session::Session;

/// Load Claude sessions from the local Claude project store.
///
/// # Errors
/// Returns an error when the Claude sessions directory cannot be read or a
/// session file cannot be parsed.
pub fn load_sessions() -> rootcause::Result<Vec<Session>> {
    let root = ytil_sys::dir::build_home_path(Agent::Claude.sessions_root_path())?;
    let session_paths = crate::agent::session_loader::find_session_paths(
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
        let claude_session = crate::agent::session_parser::claude::parse(&content)
            .attach_with(|| format!("path={}", session_path.display()))?;
        let mut session = Session::from(claude_session);
        if session.workspace.is_dir() {
            session.path = session_path;
            sessions.push(session);
        }
    }

    Ok(sessions)
}
