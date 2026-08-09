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

/// Delete selected sessions from their owning agent stores.
///
/// Every selected key is resolved and deleted independently. A failure for one
/// key does not prevent attempts for the remaining keys.
pub fn delete_sessions(home_dir: &Path, keys: &[SessionKey]) -> DeletionReport {
    let mut report = DeletionReport::default();
    for key in keys {
        match build_deletion_plan(home_dir, key) {
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
    report
}

fn build_deletion_plan(home_dir: &Path, key: &SessionKey) -> rootcause::Result<DeletionPlan> {
    match key.agent() {
        Agent::Claude => claude::build_deletion_plan(&session_root(home_dir, Agent::Claude), key),
        Agent::Codex => codex::build_deletion_plan(&session_root(home_dir, Agent::Codex), key),
        Agent::Cursor => cursor::build_deletion_plan(&session_root(home_dir, Agent::Cursor), key),
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
    use std::fmt::Write;
    use std::path::PathBuf;

    use rootcause::report;
    use rusqlite::Connection;
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
        let cursor_path = home_dir.join(".cursor/chats/cursor/store.db");
        write_file(&claude_path, "session");
        write_file(
            &codex_path,
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"codex\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
        );
        create_cursor_store(&cursor_path, "cursor");
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

    fn write_file(path: &Path, content: &str) {
        let parent = path.parent().expect("fixture path should have a parent");
        std::fs::create_dir_all(parent).expect("fixture parent should be created");
        std::fs::write(path, content).expect("fixture should be written");
    }

    fn create_cursor_store(path: &Path, session_id: &str) {
        let parent = path.parent().expect("fixture database should have a parent");
        std::fs::create_dir_all(parent).expect("fixture database parent should be created");
        let connection = Connection::open(path).expect("fixture database should open");
        connection
            .execute("create table meta (value text)", [])
            .expect("fixture metadata table should be created");
        let metadata = hex(&format!(
            r#"{{"agentId":"{session_id}","name":"Cursor Session","createdAt":1774877738013}}"#
        ));
        connection
            .execute("insert into meta (value) values (?1)", [&metadata])
            .expect("fixture metadata should be written");
    }

    fn hex(value: &str) -> String {
        let mut output = String::with_capacity(value.len().saturating_mul(2));
        for byte in value.as_bytes() {
            write!(&mut output, "{byte:02x}").expect("writing to string should not fail");
        }
        output
    }
}
