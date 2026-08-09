use std::path::Path;

use rootcause::prelude::ResultExt;
use rootcause::report;
use rusqlite::Connection;
use rusqlite::OpenFlags;
use rusqlite::OptionalExtension;

use super::DeletionPlan;
use crate::agent::session::SessionKey;

pub(super) fn build_deletion_plan(root: &Path, key: &SessionKey) -> rootcause::Result<DeletionPlan> {
    let session_paths = crate::agent::session_loader::find_session_paths(
        root,
        |entry| entry.path().file_name().is_some_and(|name| name == "store.db"),
        |_| false,
    )?;
    let mut matches = Vec::new();
    for store_db in session_paths {
        let meta_hex = read_meta_hex(&store_db)?;
        let Some(meta_hex) = meta_hex.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        let session_id = crate::agent::session_parser::cursor::parse_session_id(&meta_hex)
            .attach_with(|| format!("store_db={}", store_db.display()))?;
        if session_id == key.id() {
            matches.push(store_db.parent().map_or_else(|| store_db.clone(), Path::to_path_buf));
        }
    }
    let [path] = matches.as_slice() else {
        return Err(
            report!("selected Cursor session was not found uniquely in the session store")
                .attach(format!("session_id={}", key.id()))
                .attach(format!("matches={}", matches.len())),
        );
    };
    Ok(DeletionPlan::new(key.clone(), vec![path.clone()], 0, Vec::new()))
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
