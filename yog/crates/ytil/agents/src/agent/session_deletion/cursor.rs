use std::path::Path;

use rootcause::prelude::ResultExt;
use rootcause::report;

use super::DeletionPlan;
use crate::agent::session::SessionKey;

pub(super) fn build_deletion_plan(
    root: &Path,
    key: &SessionKey,
    selected_path: Option<&Path>,
) -> rootcause::Result<DeletionPlan> {
    if let Some(selected_path) = selected_path {
        return plan_for_selected_path(root, key, selected_path);
    }

    let session_paths = crate::agent::session_loader::find_session_paths(
        root,
        crate::agent::session_loader::cursor::is_session_file,
        |_| false,
    )?;
    let mut matches = Vec::new();
    for meta_path in session_paths {
        let Some(session_dir) = meta_path.parent() else {
            continue;
        };
        if session_dir.file_name().and_then(|name| name.to_str()) == Some(key.id()) {
            matches.push(session_dir.to_path_buf());
        }
    }
    matches.sort();
    matches.dedup();
    let [path] = matches.as_slice() else {
        return Err(
            report!("selected Cursor session was not found uniquely in the session store")
                .attach(format!("session_id={}", key.id()))
                .attach(format!("matches={}", matches.len())),
        );
    };
    Ok(DeletionPlan::new(key.clone(), vec![path.clone()], 0, Vec::new()))
}

fn plan_for_selected_path(root: &Path, key: &SessionKey, selected_path: &Path) -> rootcause::Result<DeletionPlan> {
    let root = root
        .canonicalize()
        .context("failed to resolve Cursor session store")
        .attach_with(|| format!("path={}", root.display()))?;
    let selected_path = selected_path
        .canonicalize()
        .context("failed to resolve selected Cursor session path")
        .attach_with(|| format!("path={}", selected_path.display()))?;
    if !selected_path.starts_with(&root) {
        return Err(report!("selected Cursor session path is outside the session store")
            .attach(format!("path={}", selected_path.display()))
            .attach(format!("root={}", root.display())));
    }
    if selected_path.file_name().and_then(|name| name.to_str()) != Some(key.id()) {
        return Err(report!("selected Cursor session path does not match the session id")
            .attach(format!("path={}", selected_path.display()))
            .attach(format!("session_id={}", key.id())));
    }
    if !selected_path.join("meta.json").is_file() {
        return Err(report!("selected Cursor session is missing meta.json")
            .attach(format!("path={}", selected_path.display()))
            .attach(format!("session_id={}", key.id())));
    }

    Ok(DeletionPlan::new(key.clone(), vec![selected_path], 0, Vec::new()))
}
