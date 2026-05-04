use rootcause::prelude::ResultExt;

use crate::agent::Agent;
use crate::agent::session::Session;

/// Load Codex sessions from the local Codex session store.
///
/// # Errors
/// Returns an error when the Codex sessions directory cannot be read or a
/// session file cannot be parsed.
pub fn load_sessions() -> rootcause::Result<Vec<Session>> {
    let root = ytil_sys::dir::build_home_path(Agent::Codex.sessions_root_path())?;
    let session_paths = crate::agent::session_loader::find_session_paths(
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
        let codex_session = crate::agent::session_parser::codex::parse(&content, session_name)
            .attach_with(|| format!("path={}", session_path.display()))?;
        if codex_session.is_subagent {
            continue;
        }
        let mut session = Session::from(codex_session);
        if session.workspace.is_dir() {
            session.path = session_path;
            sessions.push(session);
        }
    }

    Ok(sessions)
}
