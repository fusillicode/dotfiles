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

pub(super) fn build_deletion_plan(root: &Path, key: &SessionKey) -> rootcause::Result<DeletionPlan> {
    let root = root
        .canonicalize()
        .context("failed to resolve Codex session store")
        .attach_with(|| format!("path={}", root.display()))?;
    let session_paths = crate::agent::session_loader::find_session_paths(
        &root,
        |entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"),
        |_| false,
    )?;

    let (sessions, skipped_paths) = scan_deletion_sessions(&root, session_paths);
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
        let deletion_session = DeletionSession {
            path: resolved_path,
            parent_thread_id: metadata.parent_thread_id,
        };
        sessions
            .entry(metadata.id)
            .or_insert_with(Vec::new)
            .push(deletion_session);
    }
    (sessions, skipped_paths)
}

fn children_by_parent(sessions: &HashMap<String, Vec<DeletionSession>>) -> HashMap<&str, Vec<&str>> {
    let mut children = HashMap::new();
    for (session_id, matches) in sessions {
        for session in matches {
            if let Some(parent_id) = &session.parent_thread_id {
                children
                    .entry(parent_id.as_str())
                    .or_insert_with(Vec::new)
                    .push(session_id.as_str());
            }
        }
    }
    children
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
