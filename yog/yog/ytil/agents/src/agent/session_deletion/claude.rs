use std::path::Path;

use super::DeletionPlan;
use crate::agent::session::SessionKey;

pub(super) fn build_deletion_plan(root: &Path, key: &SessionKey) -> rootcause::Result<DeletionPlan> {
    let paths = crate::agent::session_loader::find_session_paths(
        root,
        |entry| claude_session_path(&entry.path(), key.id()),
        |_| false,
    )?;
    let [path] = paths.as_slice() else {
        return Err(
            rootcause::report!("selected Claude session was not found uniquely in the session store")
                .attach(format!("session_id={}", key.id()))
                .attach(format!("matches={}", paths.len())),
        );
    };
    Ok(DeletionPlan::new(key.clone(), vec![path.clone()], 0, Vec::new()))
}

fn claude_session_path(path: &Path, session_id: &str) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !matches!(name, "sessions-index.json" | "session.json"))
        && path.file_stem().and_then(|stem| stem.to_str()) == Some(session_id)
}
