use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use rootcause::prelude::ResultExt as _;
use rootcause::report;
use rusqlite::Connection;
use rusqlite::OptionalExtension as _;

use crate::agent::Agent;
use crate::agent::session::Session;

pub fn load_sessions() -> rootcause::Result<Vec<Session>> {
    let chats_root = ytil_sys::dir::build_home_path(Agent::Cursor.sessions_root_path())?;
    let session_paths = ytil_sys::file::find_matching_recursively_in_dir(
        &chats_root,
        |entry| entry.path().file_name().is_some_and(|name| name == "store.db"),
        |_| false,
    )?;

    let known_workspaces = load_known_workspaces()?;
    let ignored_roots = vec![ytil_sys::dir::build_home_path(Agent::Cursor.root_path())?];

    let mut sessions = Vec::new();
    for store_db in session_paths {
        let meta_hex = read_meta_hex(&store_db)?;
        let Some(meta_hex) = meta_hex.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        let strings_output = read_strings_output(&store_db)?;
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
        let mut session = crate::agent::session_parser::cursor::parse(&meta_hex, workspace)
            .attach_with(|| format!("store_db={}", store_db.display()))?;
        session.search_text =
            crate::agent::session_parser::cursor::build_search_text_from_strings(&session.name, &strings_output);
        session.updated_at = super::file_updated_at(&store_db)?.unwrap_or(session.created_at);
        session.path = store_db.parent().map_or_else(|| store_db.clone(), Path::to_path_buf);
        sessions.push(session);
    }

    Ok(sessions)
}

fn load_known_workspaces() -> rootcause::Result<Vec<PathBuf>> {
    let root = ytil_sys::dir::build_home_path(&[".cursor", "projects"])?;

    let mut workspaces = Vec::new();
    for path in ytil_sys::file::find_matching_recursively_in_dir(
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
    let connection = Connection::open(store_db)
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
