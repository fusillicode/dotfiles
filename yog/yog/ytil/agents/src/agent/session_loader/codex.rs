use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;
use rootcause::report;

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

/// A descendant-first file deletion plan for one Codex session root.
#[derive(Debug)]
pub struct DeletionPlan {
    parent_id: String,
    paths: Vec<PathBuf>,
}

impl DeletionPlan {
    pub fn parent_id(&self) -> &str {
        &self.parent_id
    }

    pub const fn descendant_count(&self) -> usize {
        self.paths.len().saturating_sub(1)
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

/// Resolve complete descendant-first deletion plans for selected Codex sessions.
///
/// # Errors
/// Returns an error when the complete local Codex store cannot establish an
/// unambiguous, acyclic, in-store parent graph.
pub fn resolve_deletion_plans<S: std::hash::BuildHasher>(
    root: &Path,
    selected_ids: &HashSet<&str, S>,
) -> rootcause::Result<Vec<DeletionPlan>> {
    if selected_ids.is_empty() {
        return Ok(Vec::new());
    }
    let root = root
        .canonicalize()
        .context("failed to resolve Codex session store")
        .attach_with(|| format!("path={}", root.display()))?;
    let session_paths = crate::agent::session_loader::find_session_paths(
        &root,
        |entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"),
        |_| false,
    )?;

    let sessions = scan_deletion_sessions(&root, session_paths)?;
    let children_by_parent = children_by_parent(&sessions);
    validate_acyclic_graph(&sessions)?;

    let mut plans = Vec::with_capacity(selected_ids.len());
    for selected_id in selected_ids {
        if !sessions.contains_key(*selected_id) {
            return Err(report!("selected Codex session was not found in the session store")
                .attach(format!("session_id={selected_id}")));
        }

        let mut paths = Vec::new();
        collect_descendant_paths(selected_id, &children_by_parent, &sessions, &mut paths);

        plans.push(DeletionPlan {
            parent_id: (*selected_id).to_owned(),
            paths,
        });
    }

    validate_non_overlapping_plans(&plans)?;
    Ok(plans)
}

#[derive(Debug)]
struct DeletionSession {
    path: PathBuf,
    parent_thread_id: Option<String>,
}

fn scan_deletion_sessions(
    root: &Path,
    session_paths: Vec<PathBuf>,
) -> rootcause::Result<HashMap<String, DeletionSession>> {
    let mut sessions = HashMap::new();
    for session_path in session_paths {
        let resolved_path = session_path
            .canonicalize()
            .context("failed to resolve Codex session file")
            .attach_with(|| format!("path={}", session_path.display()))?;
        if !resolved_path.starts_with(root) {
            return Err(report!("Codex session file is outside the session store")
                .attach(format!("path={}", resolved_path.display()))
                .attach(format!("root={}", root.display())));
        }

        let session_name = session_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let file = File::open(&session_path)
            .context("failed to open Codex session file")
            .attach_with(|| format!("path={}", session_path.display()))?;
        let metadata =
            crate::agent::session_parser::codex::parse_metadata_for_deletion(BufReader::new(file), session_name)
                .attach_with(|| format!("path={}", session_path.display()))?;
        let deletion_session = DeletionSession {
            path: session_path,
            parent_thread_id: metadata.parent_thread_id,
        };

        if sessions.insert(metadata.id.clone(), deletion_session).is_some() {
            return Err(
                report!("duplicate Codex session ID in session store").attach(format!("session_id={}", metadata.id))
            );
        }
    }
    Ok(sessions)
}

fn children_by_parent(sessions: &HashMap<String, DeletionSession>) -> HashMap<&str, Vec<&str>> {
    let mut children = HashMap::new();
    for (session_id, session) in sessions {
        if let Some(parent_id) = &session.parent_thread_id {
            children
                .entry(parent_id.as_str())
                .or_insert_with(Vec::new)
                .push(session_id.as_str());
        }
    }
    children
}

fn validate_acyclic_graph(sessions: &HashMap<String, DeletionSession>) -> rootcause::Result<()> {
    let mut states = HashMap::new();
    for session_id in sessions.keys() {
        validate_session_path_acyclic(session_id, sessions, &mut states)?;
    }
    Ok(())
}

fn validate_session_path_acyclic(
    session_id: &str,
    sessions: &HashMap<String, DeletionSession>,
    states: &mut HashMap<String, VisitState>,
) -> rootcause::Result<()> {
    let mut current_id = session_id;
    let mut visiting = Vec::new();

    loop {
        match states.get(current_id) {
            Some(VisitState::Complete) => break,
            Some(VisitState::Visiting) => {
                return Err(report!("cyclic Codex session parent graph").attach(format!("session_id={current_id}")));
            }
            None => {
                states.insert(current_id.to_owned(), VisitState::Visiting);
                visiting.push(current_id);
            }
        }

        let parent_id = sessions
            .get(current_id)
            .and_then(|session| session.parent_thread_id.as_deref())
            .filter(|parent_id| sessions.contains_key(*parent_id));
        let Some(parent_id) = parent_id else {
            break;
        };
        current_id = parent_id;
    }

    for visited_id in visiting {
        states.insert(visited_id.to_owned(), VisitState::Complete);
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum VisitState {
    Visiting,
    Complete,
}

enum TraversalStep<'a> {
    Visit(&'a str),
    Append(&'a str),
}

fn collect_descendant_paths(
    session_id: &str,
    children_by_parent: &HashMap<&str, Vec<&str>>,
    sessions: &HashMap<String, DeletionSession>,
    paths: &mut Vec<PathBuf>,
) {
    let mut steps = vec![TraversalStep::Visit(session_id)];
    while let Some(step) = steps.pop() {
        match step {
            TraversalStep::Visit(session_id) => {
                steps.push(TraversalStep::Append(session_id));
                if let Some(children) = children_by_parent.get(session_id) {
                    for child_id in children {
                        steps.push(TraversalStep::Visit(child_id));
                    }
                }
            }
            TraversalStep::Append(session_id) => {
                if let Some(session) = sessions.get(session_id) {
                    paths.push(session.path.clone());
                }
            }
        }
    }
}

fn validate_non_overlapping_plans(plans: &[DeletionPlan]) -> rootcause::Result<()> {
    let mut paths = HashSet::new();
    for plan in plans {
        for path in plan.paths() {
            if !paths.insert(path) {
                return Err(report!("selected Codex deletion plans overlap").attach(format!("path={}", path.display())));
            }
        }
    }
    Ok(())
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
    use std::collections::HashSet;

    use tempfile::tempdir;
    use test_that::prelude::*;

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

        let sessions_result = load_sessions_from_root_by_key(&root, &[SessionKey::new(Agent::Codex, "target")]);
        assert_that!(sessions_result.as_ref().map(|_| ()), ok(eq(())));
        let sessions = sessions_result.expect("target Codex session should load");

        assert_that!(sessions.len(), eq(1));
        assert_that!(sessions[0].id, eq("target"));
    }

    #[test]
    fn load_sessions_from_paths_when_workspace_is_missing_keeps_session_for_deletion() {
        let dir = tempdir().expect("tempdir should be created");
        let session_path = dir.path().join("rollout-2026-01-01-target.jsonl");
        let missing_workspace = dir.path().join("missing-workspace");
        std::fs::write(&session_path, codex_content("target", &missing_workspace))
            .expect("session fixture should be written");

        let sessions = load_sessions_from_paths(vec![session_path], |_| true).expect("session should load");

        assert_that!(sessions.len(), eq(1));
        assert_that!(sessions[0].workspace, eq(missing_workspace));
    }

    #[test]
    fn resolve_deletion_plans_when_selected_parent_has_nested_children_returns_descendant_first_plan() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        let parent = write_deletion_session(&root, "parent", None, "parent.jsonl");
        let child = write_deletion_session(&root, "child", Some("parent"), "child.jsonl");
        let grandchild = write_deletion_session(&root, "grandchild", Some("child"), "grandchild.jsonl");
        let unrelated = write_deletion_session(&root, "other", None, "other.jsonl");
        let selected = HashSet::from(["parent"]);

        let plans = resolve_deletion_plans(&root, &selected).expect("plan should resolve");

        assert_that!(plans.len(), eq(1));
        assert_that!(plans[0].descendant_count(), eq(2));
        assert_that!(
            plans[0].paths(),
            eq([
                grandchild.canonicalize().expect("path should resolve"),
                child.canonicalize().expect("path should resolve"),
                parent.canonicalize().expect("path should resolve"),
            ])
        );
        assert_that!(plans[0].paths().contains(&unrelated), eq(false));
    }

    #[test]
    fn collect_descendant_paths_when_tree_is_deep_uses_iterative_post_order() {
        let depth: usize = 10_000;
        let mut sessions = HashMap::new();
        for index in 0..depth {
            let session_id = format!("session-{index}");
            let parent_thread_id = (index > 0).then(|| format!("session-{}", index.saturating_sub(1)));
            sessions.insert(
                session_id.clone(),
                DeletionSession {
                    path: PathBuf::from(&session_id),
                    parent_thread_id,
                },
            );
        }
        let children = children_by_parent(&sessions);
        let mut paths = Vec::new();

        assert_that!(validate_acyclic_graph(&sessions), ok(eq(())));
        collect_descendant_paths("session-0", &children, &sessions, &mut paths);

        assert_that!(paths.len(), eq(depth));
        assert_that!(paths.first(), eq(Some(&PathBuf::from("session-9999"))));
        assert_that!(paths.last(), eq(Some(&PathBuf::from("session-0"))));
    }

    #[test]
    fn resolve_deletion_plans_when_selected_plans_overlap_returns_error() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        write_deletion_session(&root, "parent", None, "parent.jsonl");
        write_deletion_session(&root, "child", Some("parent"), "child.jsonl");
        let selected = HashSet::from(["parent", "child"]);

        let result = resolve_deletion_plans(&root, &selected);

        assert_that!(result, err(displays_as(contains_substring("plans overlap"))));
    }

    #[test]
    fn resolve_deletion_plans_when_selected_plans_are_independent_returns_each_plan() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        write_deletion_session(&root, "one", None, "one.jsonl");
        write_deletion_session(&root, "two", None, "two.jsonl");
        let selected = HashSet::from(["one", "two"]);

        let plans = resolve_deletion_plans(&root, &selected).expect("plans should resolve");

        assert_that!(plans.len(), eq(2));
        assert_that!(plans.iter().all(|plan| plan.descendant_count() == 0), eq(true));
    }

    #[test]
    fn resolve_deletion_plans_when_store_has_duplicate_ids_returns_error() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        write_deletion_session(&root, "parent", None, "one.jsonl");
        write_deletion_session(&root, "parent", None, "two.jsonl");
        let selected = HashSet::from(["parent"]);

        let result = resolve_deletion_plans(&root, &selected);

        assert_that!(
            result,
            err(displays_as(contains_substring("duplicate Codex session ID")))
        );
    }

    #[test]
    fn resolve_deletion_plans_when_parent_graph_is_cyclic_returns_error() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        write_deletion_session(&root, "one", Some("two"), "one.jsonl");
        write_deletion_session(&root, "two", Some("one"), "two.jsonl");
        let selected = HashSet::from(["one"]);

        let result = resolve_deletion_plans(&root, &selected);

        assert_that!(
            result,
            err(displays_as(contains_substring("cyclic Codex session parent graph")))
        );
    }

    #[test]
    fn resolve_deletion_plans_when_session_metadata_is_missing_returns_error() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        std::fs::write(root.join("invalid.jsonl"), "{\"type\":\"other\"}\n").expect("fixture should be written");
        let selected = HashSet::from(["parent"]);

        let result = resolve_deletion_plans(&root, &selected);

        assert_that!(
            result,
            err(displays_as(contains_substring("no Codex session_meta record found")))
        );
    }

    #[test]
    fn resolve_deletion_plans_when_metadata_is_invalid_returns_error() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        std::fs::write(root.join("invalid.jsonl"), "not json\n").expect("fixture should be written");
        let selected = HashSet::from(["parent"]);

        let result = resolve_deletion_plans(&root, &selected);

        assert_that!(
            result,
            err(displays_as(contains_substring(
                "failed to parse Codex session json line"
            )))
        );
    }

    #[test]
    fn resolve_deletion_plans_when_jsonl_is_malformed_after_metadata_keeps_plan_valid() {
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
        let selected = HashSet::from(["parent"]);

        let plans = resolve_deletion_plans(&root, &selected).expect("plan should resolve");

        assert_that!(
            plans[0].paths(),
            eq([parent.canonicalize().expect("path should resolve")])
        );
    }

    #[test]
    fn scan_deletion_sessions_when_path_is_outside_store_returns_error() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).expect("session root should be created");
        let outside = write_deletion_session(dir.path(), "parent", None, "outside.jsonl");
        let canonical_root = root.canonicalize().expect("root should resolve");

        let result = scan_deletion_sessions(&canonical_root, vec![outside]);

        assert_that!(
            result,
            err(displays_as(contains_substring("outside the session store")))
        );
    }

    #[test]
    fn scan_deletion_sessions_when_a_session_path_is_unreadable_returns_error() {
        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(root.join("unreadable.jsonl")).expect("fixture directory should be created");
        let canonical_root = root.canonicalize().expect("root should resolve");

        let result = scan_deletion_sessions(&canonical_root, vec![root.join("unreadable.jsonl")]);

        assert_that!(
            result,
            err(displays_as(contains_substring(
                "failed to read Codex session json line"
            )))
        );
    }

    fn codex_content(id: &str, workspace: &Path) -> String {
        format!(
            "{{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"{}\"}}}}\n",
            workspace.display()
        )
    }

    fn write_deletion_session(root: &Path, id: &str, parent_id: Option<&str>, filename: &str) -> PathBuf {
        let path = root.join(filename);
        let parent = parent_id.map_or_else(String::new, |parent_id| {
            format!(",\"parent_thread_id\":\"{parent_id}\"")
        });
        let content = format!(
            "{{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\"{parent},\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}}}\n"
        );
        std::fs::write(&path, content).expect("session fixture should be written");
        path
    }
}
