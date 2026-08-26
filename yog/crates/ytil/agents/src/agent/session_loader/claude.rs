use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;

use crate::agent::Agent;
use crate::agent::session::Session;
use crate::agent::session::SessionKey;

/// Load Claude sessions from the local Claude project store.
///
/// # Errors
/// Returns an error when the Claude sessions directory cannot be read or a
/// session file cannot be parsed.
pub fn load_sessions() -> rootcause::Result<Vec<Session>> {
    let root = ytil_sys::dir::build_home_path(Agent::Claude.sessions_root_path())?;
    let session_paths =
        crate::agent::session_loader::find_session_paths(&root, |entry| claude_session_path(&entry.path()), |_| false)?;

    load_sessions_from_paths(session_paths, |_| true)
}

/// Load only requested Claude sessions from the local Claude project store.
///
/// # Errors
/// Returns an error when a matching Claude session file cannot be read or parsed.
pub fn load_sessions_by_key(keys: &[SessionKey]) -> rootcause::Result<Vec<Session>> {
    let root = ytil_sys::dir::build_home_path(Agent::Claude.sessions_root_path())?;
    load_sessions_from_root_by_key(&root, keys)
}

fn load_sessions_from_root_by_key(root: &Path, keys: &[SessionKey]) -> rootcause::Result<Vec<Session>> {
    let requested_ids = crate::agent::session_loader::requested_ids(keys, Agent::Claude);
    if requested_ids.is_empty() {
        return Ok(Vec::new());
    }
    let session_paths = crate::agent::session_loader::find_session_paths(
        root,
        |entry| claude_session_path_matches_requested_id(&entry.path(), &requested_ids),
        |_| false,
    )?;

    load_sessions_from_paths(session_paths, |session| requested_ids.contains(session.id.as_str()))
}

fn load_sessions_from_paths(
    session_paths: Vec<PathBuf>,
    keep_session: impl Fn(&Session) -> bool,
) -> rootcause::Result<Vec<Session>> {
    let mut sessions = Vec::new();
    for session_path in session_paths {
        let content = std::fs::read_to_string(&session_path)
            .context("failed to read Claude session file")
            .attach_with(|| format!("path={}", session_path.display()))?;
        let claude_session = crate::agent::session_parser::claude::parse(&content)
            .attach_with(|| format!("path={}", session_path.display()))?;
        let session = claude_session.into_session(session_path);
        if session.workspace.is_dir() && keep_session(&session) {
            sessions.push(session);
        }
    }

    Ok(sessions)
}

fn claude_session_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !matches!(name, "sessions-index.json" | "session.json"))
}

fn claude_session_path_matches_requested_id(path: &Path, requested_ids: &HashSet<&str>) -> bool {
    claude_session_path(path)
        && path
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|stem| requested_ids.contains(stem))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_load_sessions_from_root_by_key_only_parses_matching_claude_files() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("projects");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&root).expect("session root should be created");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        std::fs::write(root.join("target.jsonl"), claude_content("target", &workspace))
            .expect("target session should be written");
        std::fs::write(root.join("other.jsonl"), "not json\n").expect("nonmatching session should be written");

        let sessions_result = load_sessions_from_root_by_key(&root, &[SessionKey::new(Agent::Claude, "target")]);
        assert_that!(sessions_result.as_ref().map(|_| ()), ok(eq(())));
        let sessions = sessions_result.expect("target Claude session should load");

        assert_that!(sessions.len(), eq(1));
        assert_that!(sessions[0].id, eq("target"));
    }

    fn claude_content(id: &str, workspace: &Path) -> String {
        format!(
            "{{\"type\":\"progress\",\"timestamp\":\"2026-03-26T16:51:01.119Z\",\"cwd\":\"{}\",\"sessionId\":\"{id}\"}}\n",
            workspace.display()
        )
    }
}
