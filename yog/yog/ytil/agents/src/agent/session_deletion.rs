use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::agent::Agent;
use crate::agent::session::SessionKey;

mod claude;
mod codex;
mod cursor;

/// A storage-specific deletion target resolved for one selected session.
#[derive(Debug)]
pub struct DeletionPlan {
    key: SessionKey,
    paths: Vec<PathBuf>,
    related_session_count: usize,
    skipped_paths: Vec<PathBuf>,
}

impl DeletionPlan {
    pub(crate) const fn new(
        key: SessionKey,
        paths: Vec<PathBuf>,
        related_session_count: usize,
        skipped_paths: Vec<PathBuf>,
    ) -> Self {
        Self {
            key,
            paths,
            related_session_count,
            skipped_paths,
        }
    }

    pub const fn key(&self) -> &SessionKey {
        &self.key
    }

    pub const fn related_session_count(&self) -> usize {
        self.related_session_count
    }
}

/// The result of resolving and deleting one selected session.
#[derive(Debug)]
pub enum DeletionOutcome {
    Deleted {
        key: SessionKey,
        related_session_count: usize,
    },
    Failed {
        key: SessionKey,
        error: rootcause::Report,
    },
}

/// Reports from files that could not safely participate in session discovery.
#[derive(Debug, Default)]
pub struct DeletionReport {
    outcomes: Vec<DeletionOutcome>,
    skipped_paths: Vec<PathBuf>,
}

impl DeletionReport {
    pub fn outcomes(&self) -> &[DeletionOutcome] {
        &self.outcomes
    }

    pub fn skipped_paths(&self) -> &[PathBuf] {
        &self.skipped_paths
    }
}

/// A listed session chosen for deletion, including its store path.
pub struct DeletionTarget {
    key: SessionKey,
    path: PathBuf,
}

impl DeletionTarget {
    #[must_use]
    pub const fn new(key: SessionKey, path: PathBuf) -> Self {
        Self { key, path }
    }

    #[must_use]
    pub const fn key(&self) -> &SessionKey {
        &self.key
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Delete selected sessions from their owning agent stores.
///
/// Every selected key is resolved and deleted independently. A failure for one
/// key does not prevent attempts for the remaining keys.
pub fn delete_sessions(home_dir: &Path, keys: &[SessionKey]) -> DeletionReport {
    let mut report = DeletionReport::default();
    for key in keys {
        apply_deletion(&mut report, home_dir, key, None);
    }
    report
}

/// Delete listed sessions using each session's already resolved store path.
pub fn delete_session_targets(home_dir: &Path, targets: &[DeletionTarget]) -> DeletionReport {
    let mut report = DeletionReport::default();
    for target in targets {
        apply_deletion(&mut report, home_dir, target.key(), Some(target.path()));
    }
    report
}

fn apply_deletion(report: &mut DeletionReport, home_dir: &Path, key: &SessionKey, selected_path: Option<&Path>) {
    match build_deletion_plan(home_dir, key, selected_path) {
        Ok(plan) => {
            let DeletionPlan {
                key,
                paths,
                related_session_count,
                skipped_paths,
            } = plan;
            report.skipped_paths.extend(skipped_paths);
            match delete_paths_in_order(&paths, delete_session_path) {
                Ok(()) => report.outcomes.push(DeletionOutcome::Deleted {
                    key,
                    related_session_count,
                }),
                Err(error) => report.outcomes.push(DeletionOutcome::Failed { key, error }),
            }
        }
        Err(error) => report.outcomes.push(DeletionOutcome::Failed {
            key: key.clone(),
            error,
        }),
    }
}

fn build_deletion_plan(
    home_dir: &Path,
    key: &SessionKey,
    selected_path: Option<&Path>,
) -> rootcause::Result<DeletionPlan> {
    match key.agent() {
        Agent::Claude => claude::build_deletion_plan(&session_root(home_dir, Agent::Claude), key),
        Agent::Codex => codex::build_deletion_plan(&session_root(home_dir, Agent::Codex), key),
        Agent::Cursor => cursor::build_deletion_plan(&session_root(home_dir, Agent::Cursor), key, selected_path),
        Agent::Gemini | Agent::Opencode => {
            Err(report!("session deletion is not supported").attach(format!("agent={}", key.agent())))
        }
    }
}

fn session_root(home_dir: &Path, agent: Agent) -> PathBuf {
    agent
        .sessions_root_path()
        .iter()
        .fold(home_dir.to_path_buf(), |path, component| path.join(component))
}

fn delete_paths_in_order(
    paths: &[PathBuf],
    mut delete_path: impl FnMut(&Path) -> rootcause::Result<()>,
) -> rootcause::Result<()> {
    for path in paths {
        delete_path(path)?;
    }
    Ok(())
}

fn delete_session_path(path: &Path) -> rootcause::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
            .context("failed to delete session directory")
            .attach_with(|| format!("path={}", path.display()))?;
    } else {
        std::fs::remove_file(path)
            .context("failed to delete session file")
            .attach_with(|| format!("path={}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rootcause::report;
    use tempfile::tempdir;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_delete_paths_in_order_when_one_plan_fails_stops_that_plan() {
        let paths = vec![PathBuf::from("child"), PathBuf::from("parent")];
        let mut deleted = Vec::new();

        let result = delete_paths_in_order(&paths, |path| {
            if path == Path::new("child") {
                Err(report!("child deletion failed"))
            } else {
                deleted.push(path.to_path_buf());
                Ok(())
            }
        });

        assert_that!(result, err(displays_as(contains_substring("child deletion failed"))));
        assert_that!(deleted, eq(Vec::<PathBuf>::new()));
    }

    #[test]
    fn test_delete_sessions_when_one_selection_fails_deletes_valid_agent_sessions() {
        let dir = tempdir().expect("tempdir should be created");
        let home_dir = dir.path();
        let claude_path = home_dir.join(".claude/projects/claude.jsonl");
        let codex_path = home_dir.join(".codex/sessions/codex.jsonl");
        let cursor_path = home_dir.join(".cursor/chats/hash/cursor/meta.json");
        write_file(&claude_path, "session");
        write_file(
            &codex_path,
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"codex\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
        );
        write_file(&cursor_path, r#"{"cwd":"/tmp"}"#);
        let keys = vec![
            SessionKey::new(Agent::Claude, "claude"),
            SessionKey::new(Agent::Codex, "codex"),
            SessionKey::new(Agent::Cursor, "cursor"),
            SessionKey::new(Agent::Claude, "missing"),
        ];

        let report = delete_sessions(home_dir, &keys);

        assert_that!(claude_path.exists(), eq(false));
        assert_that!(codex_path.exists(), eq(false));
        assert_that!(cursor_path.parent().is_some_and(Path::exists), eq(false));
        assert_that!(
            report
                .outcomes()
                .iter()
                .filter(|outcome| matches!(outcome, DeletionOutcome::Deleted { .. }))
                .count(),
            eq(3)
        );
    }

    #[test]
    fn test_delete_sessions_when_cursor_session_is_meta_json_deletes_session_dir() {
        let dir = tempdir().expect("tempdir should be created");
        let home_dir = dir.path();
        let meta_path = home_dir.join(".cursor/chats/hash/session-id/meta.json");
        write_file(&meta_path, r#"{"cwd":"/tmp"}"#);
        let keys = vec![SessionKey::new(Agent::Cursor, "session-id")];

        let report = delete_sessions(home_dir, &keys);

        assert_that!(meta_path.parent().is_some_and(Path::exists), eq(false));
        assert_that!(
            report
                .outcomes()
                .iter()
                .filter(|outcome| matches!(outcome, DeletionOutcome::Deleted { .. }))
                .count(),
            eq(1)
        );
    }

    #[test]
    fn test_delete_session_targets_when_cursor_id_is_duplicated_deletes_only_selected_path() {
        let dir = tempdir().expect("tempdir should be created");
        let home_dir = dir.path();
        let session_id = "24ba5086-7cca-419c-85c7-e9d636670fbe";
        let selected_dir = home_dir.join(".cursor/chats/hash-a").join(session_id);
        let other_dir = home_dir.join(".cursor/chats/hash-b").join(session_id);
        write_file(&selected_dir.join("meta.json"), r#"{"cwd":"/tmp/selected"}"#);
        write_file(&other_dir.join("meta.json"), r#"{"cwd":"/tmp/other"}"#);
        let target = DeletionTarget::new(SessionKey::new(Agent::Cursor, session_id), selected_dir.clone());

        let report = delete_session_targets(home_dir, &[target]);

        assert_that!(selected_dir.exists(), eq(false));
        assert_that!(other_dir.exists(), eq(true));
        assert_that!(
            report
                .outcomes()
                .iter()
                .filter(|outcome| matches!(outcome, DeletionOutcome::Deleted { .. }))
                .count(),
            eq(1)
        );
    }

    fn write_file(path: &Path, content: &str) {
        let parent = path.parent().expect("fixture path should have a parent");
        std::fs::create_dir_all(parent).expect("fixture parent should be created");
        std::fs::write(path, content).expect("fixture should be written");
    }
}
