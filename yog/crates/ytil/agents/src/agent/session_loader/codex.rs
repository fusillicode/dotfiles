use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
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
        let session_name = session_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let file = File::open(&session_path)
            .context("failed to open Codex session file")
            .attach_with(|| format!("path={}", session_path.display()))?;
        let codex_session = crate::agent::session_parser::codex::parse_preview(BufReader::new(file), session_name)
            .attach_with(|| format!("path={}", session_path.display()))?;
        if codex_session.is_subagent {
            continue;
        }
        let mut session = codex_session.into_session(session_path.clone());
        let last_prompt_file = File::open(&session_path)
            .context("failed to open Codex session for reverse prompt scan")
            .attach_with(|| format!("path={}", session_path.display()))?;
        session.last_user_prompt = crate::agent::session_parser::codex::find_last_user_prompt(last_prompt_file)
            .attach_with(|| format!("path={}", session_path.display()))?;
        session.updated_at =
            crate::agent::session_loader::file_updated_at(&session_path)?.unwrap_or(session.created_at);
        if keep_session(&session) {
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
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_load_sessions_from_root_by_key_only_when_invoked_matching_codex_files() {
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

        let sessions_result = load_sessions_from_root_by_key(&root, &[SessionKey::new(Agent::Codex, "target")]);
        assert_that!(sessions_result.as_ref().map(|_| ()), ok(eq(())));
        let sessions = sessions_result.expect("target Codex session should load");

        assert_that!(sessions.len(), eq(1));
        assert_that!(sessions[0].id, eq("target"));
    }

    #[test]
    fn test_load_sessions_from_paths_when_workspace_is_missing_keeps_session_for_deletion() {
        let dir = tempdir().expect("tempdir should be created");
        let session_path = dir.path().join("rollout-2026-01-01-target.jsonl");
        let missing_workspace = dir.path().join("missing-workspace");
        std::fs::write(&session_path, codex_content("target", &missing_workspace))
            .expect("session fixture should be written");

        let sessions = load_sessions_from_paths(vec![session_path], |_| true).expect("session should load");

        assert_that!(sessions.len(), eq(1));
        assert_that!(sessions[0].workspace, eq(missing_workspace));
    }

    fn codex_content(id: &str, workspace: &Path) -> String {
        format!(
            "{{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"{}\"}}}}\n",
            workspace.display()
        )
    }
}
