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

fn plan_for_selected_path(root: &Path, key: &SessionKey, selected_path: &Path) -> rootcause::Result<DeletionPlan> {
    let root = root
        .canonicalize()
        .context("failed to resolve Claude session store")
        .attach_with(|| format!("path={}", root.display()))?;
    let selected_path = selected_path
        .canonicalize()
        .context("failed to resolve selected Claude session path")
        .attach_with(|| format!("path={}", selected_path.display()))?;
    if !selected_path.starts_with(&root) {
        return Err(report!("selected Claude session path is outside the session store")
            .attach(format!("path={}", selected_path.display()))
            .attach(format!("root={}", root.display())));
    }
    if !selected_path.is_file() || selected_path.extension().is_none_or(|extension| extension != "jsonl") {
        return Err(report!("selected Claude session path is not a JSONL file")
            .attach(format!("path={}", selected_path.display()))
            .attach(format!("session_id={}", key.id())));
    }

    let content = std::fs::read_to_string(&selected_path)
        .context("failed to read selected Claude session file")
        .attach_with(|| format!("path={}", selected_path.display()))?;
    let session = crate::agent::session_parser::claude::parse(&content)
        .context("failed to parse selected Claude session file")
        .attach_with(|| format!("path={}", selected_path.display()))?;
    if session.id != key.id() {
        return Err(report!("selected Claude session path does not match the session id")
            .attach(format!("path={}", selected_path.display()))
            .attach(format!("session_id={}", key.id()))
            .attach(format!("metadata_id={}", session.id)));
    }

    Ok(DeletionPlan::new(key.clone(), vec![selected_path], 0, Vec::new()))
}

fn claude_session_path(path: &Path, session_id: &str) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !matches!(name, "sessions-index.json" | "session.json"))
        && path.file_stem().and_then(|stem| stem.to_str()) == Some(session_id)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use test_that::prelude::*;

    use super::*;
    use crate::agent::Agent;

    #[test]
    fn test_build_deletion_plan_when_selected_path_has_matching_metadata_uses_selected_path() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("projects");
        std::fs::create_dir_all(&root).expect("session root should be created");
        let selected_path = root.join("selected.jsonl");
        let matching_filename_path = root.join("target.jsonl");
        std::fs::write(&selected_path, claude_content("target")).expect("selected session should be written");
        std::fs::write(&matching_filename_path, claude_content("other")).expect("other session should be written");
        let key = SessionKey::new(Agent::Claude, "target");

        let plan = build_deletion_plan(&root, &key, Some(&selected_path)).expect("plan should resolve");

        assert_that!(
            plan.paths,
            eq([selected_path.canonicalize().expect("path should resolve")])
        );
    }

    #[test]
    fn test_build_deletion_plan_when_selected_path_metadata_differs_from_key_rejects_path() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("projects");
        std::fs::create_dir_all(&root).expect("session root should be created");
        let selected_path = root.join("selected.jsonl");
        std::fs::write(&selected_path, claude_content("other")).expect("selected session should be written");
        let key = SessionKey::new(Agent::Claude, "target");

        let result = build_deletion_plan(&root, &key, Some(&selected_path));

        assert_that!(
            result,
            err(displays_as(contains_substring(
                "selected Claude session path does not match the session id"
            )))
        );
    }

    fn claude_content(id: &str) -> String {
        format!(
            "{{\"type\":\"progress\",\"timestamp\":\"2026-03-26T16:51:01.119Z\",\"cwd\":\"/tmp\",\"sessionId\":\"{id}\"}}\n"
        )
    }
}
