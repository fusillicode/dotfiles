use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;
use rootcause::report;

use super::DeletionPlan;
use crate::agent::session::SessionKey;
use crate::agent::session_parser::codex::CodexSessionMetadata;

pub(super) fn build_deletion_plan(
    root: &Path,
    key: &SessionKey,
    selected_path: Option<&Path>,
) -> rootcause::Result<DeletionPlan> {
    let root = root
        .canonicalize()
        .context("failed to resolve Codex session store")
        .attach_with(|| format!("path={}", root.display()))?;
    let selected_path = selected_path
        .map(|path| canonicalize_selected_path(&root, path))
        .transpose()?;
    let session_paths = crate::agent::session_loader::find_session_paths(
        &root,
        |entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"),
        |_| false,
    )?;

    let (sessions, skipped_paths) = scan_deletion_sessions(&root, session_paths);
    if let Some(selected_path) = selected_path.as_deref() {
        validate_selected_path(&sessions, key, selected_path)?;
    }
    let children_by_parent = children_by_parent(&sessions);
    let paths = collect_descendant_paths(key.id(), &children_by_parent, &sessions)?;
    let related_session_count = paths.len().saturating_sub(1);
    Ok(DeletionPlan::new(
        key.clone(),
        paths,
        related_session_count,
        skipped_paths,
    ))
}

#[derive(Debug)]
struct DeletionSession {
    path: PathBuf,
    parent_thread_id: Option<String>,
    is_subagent: bool,
}

fn scan_deletion_sessions(
    root: &Path,
    session_paths: Vec<PathBuf>,
) -> (HashMap<String, Vec<DeletionSession>>, Vec<PathBuf>) {
    let mut sessions = HashMap::new();
    let mut skipped_paths = Vec::new();
    for session_path in session_paths {
        let Ok(resolved_path) = session_path.canonicalize() else {
            skipped_paths.push(session_path);
            continue;
        };
        if !resolved_path.starts_with(root) {
            skipped_paths.push(session_path);
            continue;
        }

        let session_name = session_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let metadata = File::open(&session_path)
            .map(BufReader::new)
            .map_err(rootcause::Report::from)
            .and_then(|file| crate::agent::session_parser::codex::parse_metadata_for_deletion(file, session_name));
        let Ok(metadata) = metadata else {
            skipped_paths.push(session_path);
            continue;
        };
        let CodexSessionMetadata {
            id,
            parent_thread_id,
            is_subagent,
        } = metadata;
        let deletion_session = DeletionSession {
            path: resolved_path,
            parent_thread_id,
            is_subagent,
        };
        sessions.entry(id).or_insert_with(Vec::new).push(deletion_session);
    }
    (sessions, skipped_paths)
}

fn children_by_parent(sessions: &HashMap<String, Vec<DeletionSession>>) -> HashMap<&str, Vec<&str>> {
    let mut children = HashMap::new();
    for (session_id, matches) in sessions {
        for session in matches {
            if session.is_subagent
                && let Some(parent_id) = &session.parent_thread_id
            {
                children
                    .entry(parent_id.as_str())
                    .or_insert_with(Vec::new)
                    .push(session_id.as_str());
            }
        }
    }
    children
}

fn canonicalize_selected_path(root: &Path, selected_path: &Path) -> rootcause::Result<PathBuf> {
    let selected_path = selected_path
        .canonicalize()
        .context("failed to resolve selected Codex session path")
        .attach_with(|| format!("path={}", selected_path.display()))?;
    if !selected_path.starts_with(root) {
        return Err(report!("selected Codex session path is outside the session store")
            .attach(format!("path={}", selected_path.display()))
            .attach(format!("root={}", root.display())));
    }
    if !selected_path.is_file() || selected_path.extension().is_none_or(|extension| extension != "jsonl") {
        return Err(report!("selected Codex session path is not a JSONL file")
            .attach(format!("path={}", selected_path.display())));
    }
    Ok(selected_path)
}

fn validate_selected_path(
    sessions: &HashMap<String, Vec<DeletionSession>>,
    key: &SessionKey,
    selected_path: &Path,
) -> rootcause::Result<()> {
    let Some(matches) = sessions.get(key.id()) else {
        return Err(report!("selected Codex session path does not match the session id")
            .attach(format!("path={}", selected_path.display()))
            .attach(format!("session_id={}", key.id())));
    };
    if matches.iter().any(|session| session.path == selected_path) {
        Ok(())
    } else {
        Err(report!("selected Codex session path does not match the session id")
            .attach(format!("path={}", selected_path.display()))
            .attach(format!("session_id={}", key.id())))
    }
}

fn collect_descendant_paths(
    session_id: &str,
    children_by_parent: &HashMap<&str, Vec<&str>>,
    sessions: &HashMap<String, Vec<DeletionSession>>,
) -> rootcause::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut complete = HashSet::new();
    let mut visiting = HashSet::new();
    let mut steps = vec![(session_id, false)];
    while let Some(step) = steps.pop() {
        let (current_id, append) = step;
        if append {
            visiting.remove(current_id);
            complete.insert(current_id);
            let session = unique_session(current_id, sessions)?;
            paths.push(session.path.clone());
            continue;
        }
        if complete.contains(current_id) {
            continue;
        }
        if !visiting.insert(current_id) {
            return Err(report!("cyclic Codex session parent graph").attach(format!("session_id={current_id}")));
        }
        unique_session(current_id, sessions)?;
        steps.push((current_id, true));
        if let Some(children) = children_by_parent.get(current_id) {
            for child_id in children {
                steps.push((child_id, false));
            }
        }
    }
    Ok(paths)
}

fn unique_session<'a>(
    session_id: &str,
    sessions: &'a HashMap<String, Vec<DeletionSession>>,
) -> rootcause::Result<&'a DeletionSession> {
    let Some(matches) = sessions.get(session_id) else {
        return Err(report!("selected Codex session was not found in the session store")
            .attach(format!("session_id={session_id}")));
    };
    let [session] = matches.as_slice() else {
        return Err(report!("duplicate Codex session ID in readable descendant closure")
            .attach(format!("session_id={session_id}")));
    };
    Ok(session)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::tempdir;
    use test_that::prelude::*;

    use super::*;
    use crate::agent::Agent;

    #[test]
    fn test_build_deletion_plan_when_selected_parent_has_nested_children_returns_descendant_first_paths() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        let parent = write_deletion_session(&root, "parent", None, "parent.jsonl");
        let child = write_deletion_session(&root, "child", Some("parent"), "child.jsonl");
        let grandchild = write_deletion_session(&root, "grandchild", Some("child"), "grandchild.jsonl");
        let unrelated = write_deletion_session(&root, "other", None, "other.jsonl");
        let key = SessionKey::new(Agent::Codex, "parent");

        let plan = build_deletion_plan(&root, &key, None).expect("plan should resolve");

        assert_that!(plan.related_session_count(), eq(2));
        assert_that!(
            plan.paths,
            eq([
                grandchild.canonicalize().expect("path should resolve"),
                child.canonicalize().expect("path should resolve"),
                parent.canonicalize().expect("path should resolve"),
            ])
        );
        assert_that!(plan.paths.contains(&unrelated), eq(false));
    }

    #[test]
    fn test_build_deletion_plan_when_parent_id_lacks_subagent_marker_ignores_session() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        let parent = write_deletion_session(&root, "parent", None, "parent.jsonl");
        let unrelated = write_deletion_session_with_subagent(&root, "child", Some("parent"), "child.jsonl", false);
        let key = SessionKey::new(Agent::Codex, "parent");

        let plan = build_deletion_plan(&root, &key, None).expect("plan should resolve");

        assert_that!(plan.related_session_count(), eq(0));
        assert_that!(plan.paths, eq([parent.canonicalize().expect("path should resolve")]));
        assert_that!(plan.paths.contains(&unrelated), eq(false));
    }

    #[test]
    fn test_build_deletion_plan_when_selected_path_metadata_differs_from_key_rejects_path() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        let selected_path = write_deletion_session(&root, "other", None, "selected.jsonl");
        write_deletion_session(&root, "target", None, "target.jsonl");
        let key = SessionKey::new(Agent::Codex, "target");

        let result = build_deletion_plan(&root, &key, Some(&selected_path));

        assert_that!(
            result,
            err(displays_as(contains_substring(
                "selected Codex session path does not match the session id"
            )))
        );
    }

    #[test]
    fn test_collect_descendant_paths_when_tree_is_deep_uses_iterative_post_order() {
        let depth: usize = 10_000;
        let mut sessions = HashMap::new();
        for index in 0..depth {
            let session_id = format!("session-{index}");
            let parent_thread_id = (index > 0).then(|| format!("session-{}", index.saturating_sub(1)));
            sessions.insert(
                session_id.clone(),
                vec![DeletionSession {
                    path: PathBuf::from(&session_id),
                    parent_thread_id,
                    is_subagent: index > 0,
                }],
            );
        }
        let children = children_by_parent(&sessions);

        let paths = collect_descendant_paths("session-0", &children, &sessions).expect("paths should resolve");

        assert_that!(paths.len(), eq(depth));
        assert_that!(paths.first(), eq(Some(&PathBuf::from("session-9999"))));
        assert_that!(paths.last(), eq(Some(&PathBuf::from("session-0"))));
    }

    #[test]
    fn test_build_deletion_plan_when_store_has_duplicate_ids_returns_error() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        write_deletion_session(&root, "parent", None, "one.jsonl");
        write_deletion_session(&root, "parent", None, "two.jsonl");
        let key = SessionKey::new(Agent::Codex, "parent");

        let result = build_deletion_plan(&root, &key, None);

        assert_that!(
            result,
            err(displays_as(contains_substring("duplicate Codex session ID")))
        );
    }

    #[test]
    fn test_build_deletion_plan_when_parent_graph_is_cyclic_returns_error() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        write_deletion_session(&root, "one", Some("two"), "one.jsonl");
        write_deletion_session(&root, "two", Some("one"), "two.jsonl");
        let key = SessionKey::new(Agent::Codex, "one");

        let result = build_deletion_plan(&root, &key, None);

        assert_that!(
            result,
            err(displays_as(contains_substring("cyclic Codex session parent graph")))
        );
    }

    #[test]
    fn test_build_deletion_plan_when_session_metadata_is_missing_skips_the_file() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        let invalid = root.join("invalid.jsonl");
        std::fs::write(&invalid, "{\"type\":\"other\"}\n").expect("fixture should be written");
        let parent = write_deletion_session(&root, "parent", None, "parent.jsonl");
        let key = SessionKey::new(Agent::Codex, "parent");

        let plan = build_deletion_plan(&root, &key, None).expect("plan should resolve");

        assert_that!(plan.paths, eq([parent.canonicalize().expect("path should resolve")]));
        assert_that!(
            plan.skipped_paths,
            eq([invalid.canonicalize().expect("path should resolve")])
        );
    }

    #[test]
    fn test_build_deletion_plan_when_metadata_is_invalid_skips_the_file() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        let invalid = root.join("invalid.jsonl");
        std::fs::write(&invalid, "not json\n").expect("fixture should be written");
        let parent = write_deletion_session(&root, "parent", None, "parent.jsonl");
        let key = SessionKey::new(Agent::Codex, "parent");

        let plan = build_deletion_plan(&root, &key, None).expect("plan should resolve");

        assert_that!(plan.paths, eq([parent.canonicalize().expect("path should resolve")]));
        assert_that!(
            plan.skipped_paths,
            eq([invalid.canonicalize().expect("path should resolve")])
        );
    }

    #[test]
    fn test_build_deletion_plan_when_jsonl_is_malformed_after_metadata_keeps_plan_valid() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        let parent = write_deletion_session(&root, "parent", None, "parent.jsonl");
        std::fs::write(
            &parent,
            format!(
                "{}not json\n",
                std::fs::read_to_string(&parent).expect("fixture should be read")
            ),
        )
        .expect("fixture should be updated");
        let key = SessionKey::new(Agent::Codex, "parent");

        let plan = build_deletion_plan(&root, &key, None).expect("plan should resolve");

        assert_that!(plan.paths, eq([parent.canonicalize().expect("path should resolve")]));
    }

    #[test]
    fn test_scan_deletion_sessions_when_path_is_outside_store_skips_the_file() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        let outside = write_deletion_session(dir.path(), "parent", None, "outside.jsonl");
        let canonical_root = root.canonicalize().expect("root should resolve");

        let (sessions, skipped_paths) = scan_deletion_sessions(&canonical_root, vec![outside.clone()]);

        assert_that!(sessions.is_empty(), eq(true));
        assert_that!(skipped_paths, eq([outside]));
    }

    #[test]
    fn test_scan_deletion_sessions_when_a_session_path_is_unreadable_skips_the_file() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(root.join("unreadable.jsonl")).expect("fixture directory should be created");
        let canonical_root = root.canonicalize().expect("root should resolve");
        let unreadable = root.join("unreadable.jsonl");

        let (sessions, skipped_paths) = scan_deletion_sessions(&canonical_root, vec![unreadable.clone()]);

        assert_that!(sessions.is_empty(), eq(true));
        assert_that!(skipped_paths, eq([unreadable]));
    }

    fn write_deletion_session(root: &Path, id: &str, parent_id: Option<&str>, filename: &str) -> PathBuf {
        write_deletion_session_with_subagent(root, id, parent_id, filename, parent_id.is_some())
    }

    fn write_deletion_session_with_subagent(
        root: &Path,
        id: &str,
        parent_id: Option<&str>,
        filename: &str,
        is_subagent: bool,
    ) -> PathBuf {
        let path = root.join(filename);
        let parent = parent_id.map_or_else(String::new, |parent_id| {
            format!(",\"parent_thread_id\":\"{parent_id}\"")
        });
        let source = if is_subagent {
            ",\"source\":{\"subagent\":{}}".to_owned()
        } else {
            String::new()
        };
        let content = format!(
            "{{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\"{parent},\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"{source}}}}}\n"
        );
        std::fs::write(&path, content).expect("session fixture should be written");
        path
    }
}
