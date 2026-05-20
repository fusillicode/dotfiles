use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use rootcause::prelude::ResultExt;
use rootcause::report;
use rusqlite::Connection;
use rusqlite::OpenFlags;
use rusqlite::OptionalExtension;

use crate::agent::Agent;
use crate::agent::session::Session;
use crate::agent::session::SessionKey;

/// Load Cursor agent sessions from local Cursor chat databases.
///
/// # Errors
/// Returns an error when Cursor metadata cannot be read or parsed.
pub fn load_sessions() -> rootcause::Result<Vec<Session>> {
    let chats_root = ytil_sys::dir::build_home_path(Agent::Cursor.sessions_root_path())?;
    let session_paths = crate::agent::session_loader::find_session_paths(
        &chats_root,
        |entry| entry.path().file_name().is_some_and(|name| name == "store.db"),
        |_| false,
    )?;

    load_sessions_from_paths(session_paths, read_strings_output, None)
}

/// Load only requested Cursor sessions from local Cursor chat databases.
///
/// # Errors
/// Returns an error when a matching Cursor session cannot be read or parsed.
pub fn load_sessions_by_key(keys: &[SessionKey]) -> rootcause::Result<Vec<Session>> {
    let requested_ids = crate::agent::session_loader::requested_ids(keys, Agent::Cursor);
    if requested_ids.is_empty() {
        return Ok(Vec::new());
    }
    let chats_root = ytil_sys::dir::build_home_path(Agent::Cursor.sessions_root_path())?;
    let session_paths = crate::agent::session_loader::find_session_paths(
        &chats_root,
        |entry| entry.path().file_name().is_some_and(|name| name == "store.db"),
        |_| false,
    )?;

    load_sessions_from_paths(session_paths, read_strings_output, Some(&requested_ids))
}

fn load_sessions_from_paths(
    session_paths: Vec<PathBuf>,
    mut read_strings: impl FnMut(&Path) -> rootcause::Result<String>,
    requested_ids: Option<&std::collections::HashSet<&str>>,
) -> rootcause::Result<Vec<Session>> {
    let known_workspaces = load_known_workspaces()?;
    let ignored_roots = vec![ytil_sys::dir::build_home_path(Agent::Cursor.root_path())?];

    let mut sessions = Vec::new();
    for store_db in session_paths {
        let meta_hex = read_meta_hex(&store_db)?;
        let Some(meta_hex) = meta_hex.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        if let Some(requested_ids) = requested_ids {
            let session_id = crate::agent::session_parser::cursor::parse_session_id(&meta_hex)
                .attach_with(|| format!("store_db={}", store_db.display()))?;
            if !requested_ids.contains(session_id.as_str()) {
                continue;
            }
        }
        let strings_output = read_strings(&store_db)?;
        let Some(workspace) = crate::agent::session_parser::cursor::extract_cursor_workspace_from_strings(
            &strings_output,
            &known_workspaces,
            &ignored_roots,
        ) else {
            continue;
        };
        if !workspace.is_dir() {
            continue;
        }
        let mut cursor_session = crate::agent::session_parser::cursor::parse(&meta_hex, workspace)
            .attach_with(|| format!("store_db={}", store_db.display()))?;
        cursor_session.search_text =
            crate::agent::session_parser::cursor::build_search_text_from_strings(&cursor_session.name, &strings_output);
        cursor_session.updated_at =
            crate::agent::session_loader::file_updated_at(&store_db)?.unwrap_or(cursor_session.created_at);
        let path = store_db.parent().map_or_else(|| store_db.clone(), Path::to_path_buf);
        sessions.push(cursor_session.into_session(path));
    }

    Ok(sessions)
}

fn load_known_workspaces() -> rootcause::Result<Vec<PathBuf>> {
    let root = ytil_sys::dir::build_home_path(&[".cursor", "projects"])?;

    let mut workspaces = Vec::new();
    for path in crate::agent::session_loader::find_session_paths(
        &root,
        |entry| {
            entry
                .path()
                .file_name()
                .is_some_and(|name| name == ".workspace-trusted")
        },
        |_| false,
    )? {
        let content = std::fs::read_to_string(&path)
            .context("failed to read Cursor workspace marker")
            .attach_with(|| format!("path={}", path.display()))?;
        let trimmed = content.trim();
        if trimmed.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(trimmed);
        if candidate.is_dir() {
            workspaces.push(candidate);
        }
    }

    workspaces.sort();
    workspaces.dedup();

    Ok(workspaces)
}

fn read_meta_hex(store_db: &Path) -> rootcause::Result<Option<String>> {
    let connection = Connection::open_with_flags(store_db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("failed to open Cursor store db")
        .attach_with(|| format!("store_db={}", store_db.display()))?;
    Ok(connection
        .query_row("select value from meta limit 1", [], |row| row.get::<_, String>(0))
        .optional()
        .context("failed to query Cursor session metadata")
        .attach_with(|| format!("store_db={}", store_db.display()))?)
}

fn read_strings_output(store_db: &Path) -> rootcause::Result<String> {
    let output = Command::new("strings")
        .arg(store_db)
        .output()
        .context("failed to run strings for Cursor store db")
        .attach_with(|| format!("store_db={}", store_db.display()))?;

    if !output.status.success() {
        return Err(report!("strings exited with non-zero status")
            .attach(format!("store_db={}", store_db.display()))
            .attach(format!("status={}", output.status)));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fmt::Write;

    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_load_sessions_from_paths_by_key_runs_strings_only_for_matching_cursor_db() {
        let dir = tempdir().expect("tempdir should be created");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        let target_db = dir.path().join("target").join("store.db");
        let other_db = dir.path().join("other").join("store.db");
        create_store_db(&target_db, "target");
        create_store_db(&other_db, "other");
        let keys = vec![SessionKey::new(Agent::Cursor, "target")];
        let requested_ids = crate::agent::session_loader::requested_ids(&keys, Agent::Cursor);
        let strings_calls = Cell::new(0);

        assert2::assert!(let Ok(sessions) = load_sessions_from_paths(
            vec![target_db, other_db],
            |_| {
                strings_calls.set(strings_calls.get() + 1);
                Ok(workspace.display().to_string())
            },
            Some(&requested_ids),
        ));

        pretty_assertions::assert_eq!(strings_calls.get(), 1);
        pretty_assertions::assert_eq!(sessions.len(), 1);
        pretty_assertions::assert_eq!(sessions[0].id, "target");
    }

    fn create_store_db(path: &Path, session_id: &str) {
        let parent = path.parent().expect("test db path should have parent");
        std::fs::create_dir_all(parent).expect("test db parent should be created");
        let connection = Connection::open(path).expect("test db should open");
        connection
            .execute("create table meta (value text)", [])
            .expect("meta table should be created");
        let meta = hex(&format!(
            r#"{{"agentId":"{session_id}","name":"Cursor Session","createdAt":1774877738013}}"#
        ));
        connection
            .execute("insert into meta (value) values (?1)", [&meta])
            .expect("meta row should be inserted");
    }

    fn hex(value: &str) -> String {
        let mut out = String::with_capacity(value.len().saturating_mul(2));
        for byte in value.as_bytes() {
            write!(&mut out, "{byte:02x}").expect("writing to string should not fail");
        }
        out
    }
}
