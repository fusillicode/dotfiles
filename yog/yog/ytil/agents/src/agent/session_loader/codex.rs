use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;

use crate::agent::Agent;
use crate::agent::session::Session;
use crate::agent::session::SessionKey;

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

    load_sessions_from_paths(session_paths, |_| true)
}

/// Load only requested Codex sessions from the local Codex session store.
///
/// # Errors
/// Returns an error when a matching Codex session file cannot be read or parsed.
pub fn load_sessions_by_key(keys: &[SessionKey]) -> rootcause::Result<Vec<Session>> {
    let root = ytil_sys::dir::build_home_path(Agent::Codex.sessions_root_path())?;
    load_sessions_from_root_by_key(&root, keys)
}

fn load_sessions_from_root_by_key(root: &Path, keys: &[SessionKey]) -> rootcause::Result<Vec<Session>> {
    let requested_ids = crate::agent::session_loader::requested_ids(keys, Agent::Codex);
    if requested_ids.is_empty() {
        return Ok(Vec::new());
    }
    let session_paths = crate::agent::session_loader::find_session_paths(
        root,
        |entry| codex_session_path_matches_requested_id(&entry.path(), &requested_ids),
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
        let session = codex_session.into_session(session_path);
        if session.workspace.is_dir() && keep_session(&session) {
            sessions.push(session);
        }
    }

    Ok(sessions)
}

fn codex_session_path_matches_requested_id(path: &Path, requested_ids: &HashSet<&str>) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
        && path.file_stem().and_then(|name| name.to_str()).is_some_and(|stem| {
            requested_ids
                .iter()
                .any(|id| stem == *id || stem.strip_suffix(id).is_some_and(|prefix| prefix.ends_with('-')))
        })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_load_sessions_from_root_by_key_only_parses_matching_codex_files() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&root).expect("session root should be created");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        std::fs::write(
            root.join("rollout-2026-01-01-target.jsonl"),
            codex_content("target", &workspace),
        )
        .expect("target session should be written");
        std::fs::write(root.join("rollout-2026-01-01-other.jsonl"), "not json\n")
            .expect("nonmatching session should be written");

        assert2::assert!(
            let Ok(sessions) = load_sessions_from_root_by_key(&root, &[SessionKey::new(Agent::Codex, "target")])
        );

        pretty_assertions::assert_eq!(sessions.len(), 1);
        pretty_assertions::assert_eq!(sessions[0].id, "target");
    }

    fn codex_content(id: &str, workspace: &Path) -> String {
        format!(
            "{{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"{}\"}}}}\n",
            workspace.display()
        )
    }
}
