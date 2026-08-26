use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use crate::agent::Agent;
use crate::agent::session::Session;
use crate::agent::session::SessionKey;

/// Load Cursor agent sessions from local Cursor chat metadata.
///
/// Unreadable or invalid `meta.json` files are skipped.
///
/// # Errors
/// Returns an error when the Cursor session store cannot be enumerated.
pub fn load_sessions() -> rootcause::Result<Vec<Session>> {
    let chats_root = ytil_sys::dir::build_home_path(Agent::Cursor.sessions_root_path())?;
    let session_paths = crate::agent::session_loader::find_session_paths(&chats_root, is_session_file, |_| false)?;

    Ok(load_sessions_from_paths(&session_paths, None))
}

/// Load only requested Cursor sessions from local Cursor chat metadata.
///
/// Unreadable or invalid matching `meta.json` files are skipped.
///
/// # Errors
/// Returns an error when the Cursor session store cannot be enumerated.
pub fn load_sessions_by_key(keys: &[SessionKey]) -> rootcause::Result<Vec<Session>> {
    let requested_ids = crate::agent::session_loader::requested_ids(keys, Agent::Cursor);
    if requested_ids.is_empty() {
        return Ok(Vec::new());
    }
    let chats_root = ytil_sys::dir::build_home_path(Agent::Cursor.sessions_root_path())?;
    let session_paths = crate::agent::session_loader::find_session_paths(&chats_root, is_session_file, |_| false)?;

    Ok(load_sessions_from_paths(&session_paths, Some(&requested_ids)))
}

pub(crate) fn is_session_file(entry: &std::fs::DirEntry) -> bool {
    entry.path().file_name().is_some_and(|name| name == "meta.json")
}

fn load_sessions_from_paths(session_paths: &[PathBuf], requested_ids: Option<&HashSet<&str>>) -> Vec<Session> {
    session_paths
        .iter()
        .filter_map(|meta_path| load_session_from_meta_json(meta_path, requested_ids))
        .collect()
}

fn load_session_from_meta_json(meta_path: &Path, requested_ids: Option<&HashSet<&str>>) -> Option<Session> {
    let session_dir = meta_path
        .parent()
        .map_or_else(|| meta_path.to_path_buf(), Path::to_path_buf);
    let session_id = session_dir.file_name().and_then(|name| name.to_str())?;
    if requested_ids.is_some_and(|ids| !ids.contains(session_id)) {
        return None;
    }

    let json = std::fs::read_to_string(meta_path).ok()?;
    let mut cursor_session =
        crate::agent::session_parser::cursor::parse_chat_meta(&json, session_id.to_owned()).ok()?;
    if !cursor_session.has_conversation || !cursor_session.workspace.is_dir() {
        return None;
    }

    let prompts = read_prompt_history(&session_dir);
    cursor_session.search_text =
        crate::agent::session_parser::cursor::build_search_text_from_prompts(&cursor_session.name, &prompts);
    let mut session = cursor_session.into_session(session_dir);
    // Cursor writes `prompt_history.json` newest-first.
    session.last_user_prompt = prompts.iter().find(|prompt| !prompt.trim().is_empty()).cloned();
    Some(session)
}

fn read_prompt_history(session_dir: &Path) -> Vec<String> {
    let path = session_dir.join("prompt_history.json");
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_load_sessions_from_paths_by_key_when_invoked_loads_only_matching_cursor_meta() {
        let dir = tempdir().expect("tempdir should be created");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        let target_meta = dir.path().join("target").join("meta.json");
        let other_meta = dir.path().join("other").join("meta.json");
        write_meta_json(&target_meta, &workspace, true, "Target");
        write_meta_json(&other_meta, &workspace, true, "Other");
        let keys = vec![SessionKey::new(Agent::Cursor, "target")];
        let requested_ids = crate::agent::session_loader::requested_ids(&keys, Agent::Cursor);

        let sessions = load_sessions_from_paths(&[target_meta, other_meta], Some(&requested_ids));

        assert_that!(sessions.len(), eq(1));
        assert_that!(sessions[0].id, eq("target"));
        assert_that!(sessions[0].name, eq("Target"));
    }

    #[test]
    fn test_load_sessions_from_paths_when_meta_json_has_cwd_uses_workspace() {
        let dir = tempdir().expect("tempdir should be created");
        let workspace = dir.path().join("work").join("pws-api");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        let meta_path = dir
            .path()
            .join("chats")
            .join("hash")
            .join("session-id")
            .join("meta.json");
        write_meta_json(&meta_path, &workspace, true, "Status Line");
        std::fs::write(
            meta_path
                .parent()
                .expect("meta.json should have a parent")
                .join("prompt_history.json"),
            r#"["first prompt"]"#,
        )
        .expect("prompt history should be written");

        let sessions = load_sessions_from_paths(&[meta_path], None);

        assert_that!(sessions.len(), eq(1));
        assert_that!(sessions[0].id, eq("session-id"));
        assert_that!(sessions[0].name, eq("Status Line"));
        assert_that!(sessions[0].workspace, eq(workspace));
        assert_that!(sessions[0].last_user_prompt.as_deref(), eq(Some("first prompt")));
    }

    #[test]
    fn test_load_sessions_from_paths_when_prompt_history_is_newest_first_sets_last_user_prompt_to_newest() {
        let dir = tempdir().expect("tempdir should be created");
        let workspace = dir.path().join("work").join("pws-api");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        let meta_path = dir.path().join("session-id").join("meta.json");
        write_meta_json(&meta_path, &workspace, true, "Status Line");
        std::fs::write(
            meta_path
                .parent()
                .expect("meta.json should have a parent")
                .join("prompt_history.json"),
            r#"["newest", "oldest"]"#,
        )
        .expect("prompt history should be written");

        let sessions = load_sessions_from_paths(&[meta_path], None);

        assert_that!(sessions.len(), eq(1));
        assert_that!(sessions[0].last_user_prompt.as_deref(), eq(Some("newest")));
    }

    #[test]
    fn test_load_sessions_from_paths_when_meta_json_has_no_conversation_skips_session() {
        let dir = tempdir().expect("tempdir should be created");
        let workspace = dir.path().join("work").join("pws-api");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        let meta_path = dir
            .path()
            .join("chats")
            .join("hash")
            .join("session-id")
            .join("meta.json");
        write_meta_json(&meta_path, &workspace, false, "Empty");

        let sessions = load_sessions_from_paths(&[meta_path], None);

        assert_that!(sessions.len(), eq(0));
    }

    #[test]
    fn test_load_sessions_from_paths_when_one_meta_json_is_invalid_skips_and_loads_valid() {
        let dir = tempdir().expect("tempdir should be created");
        let workspace = dir.path().join("work").join("pws-api");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        let invalid_meta = dir.path().join("invalid").join("meta.json");
        let valid_meta = dir.path().join("valid").join("meta.json");
        let parent = invalid_meta.parent().expect("invalid meta.json should have a parent");
        std::fs::create_dir_all(parent).expect("invalid session dir should be created");
        std::fs::write(&invalid_meta, r#"{"hasConversation":false}"#).expect("invalid meta.json should be written");
        write_meta_json(&valid_meta, &workspace, true, "Valid");

        let sessions = load_sessions_from_paths(&[invalid_meta, valid_meta], None);

        assert_that!(sessions.len(), eq(1));
        assert_that!(sessions[0].id, eq("valid"));
        assert_that!(sessions[0].name, eq("Valid"));
    }

    fn write_meta_json(path: &Path, workspace: &Path, has_conversation: bool, title: &str) {
        let parent = path.parent().expect("meta.json should have a parent");
        std::fs::create_dir_all(parent).expect("session dir should be created");
        std::fs::write(
            path,
            format!(
                r#"{{"schemaVersion":1,"createdAtMs":1774877738013,"hasConversation":{has_conversation},"title":"{title}","updatedAtMs":1774877739013,"cwd":"{}"}}"#,
                workspace.display()
            ),
        )
        .expect("meta.json should be written");
    }
}
