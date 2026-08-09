use std::collections::HashSet;
use std::path::Path;

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_agents::agent::Agent;

use super::list::RenderableSession;

pub(super) fn delete_selected_sessions(selected: &[RenderableSession]) -> rootcause::Result<()> {
    let codex_ids = selected
        .iter()
        .filter(|session| session.session.agent == Agent::Codex)
        .map(|session| session.session.id.as_str())
        .collect::<HashSet<_>>();
    let codex_plans = if codex_ids.is_empty() {
        Ok(Vec::new())
    } else {
        ytil_sys::dir::build_home_path(Agent::Codex.sessions_root_path())
            .and_then(|root| ytil_agents::agent::session_loader::codex::resolve_deletion_plans(&root, &codex_ids))
    };
    let mut failures = Vec::new();

    for session in selected.iter().filter(|session| session.session.agent != Agent::Codex) {
        if let Err(error) = delete_session_path(&session.session.path) {
            failures.push(format!("{}: {error}", session.session.id));
        } else {
            println!("{} {session}", "Deleted".red().bold());
        }
    }

    match codex_plans {
        Ok(plans) => delete_codex_plans(plans, selected, &mut failures),
        Err(error) if !codex_ids.is_empty() => failures.push(format!("Codex preflight failed: {error}")),
        Err(_) => {}
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(report!("session deletion failed").attach(failures.join("\n")))
    }
}

fn delete_codex_plans(
    plans: Vec<ytil_agents::agent::session_loader::codex::DeletionPlan>,
    selected: &[RenderableSession],
    failures: &mut Vec<String>,
) {
    for plan in plans {
        let Some(parent) = selected
            .iter()
            .find(|session| session.session.agent == Agent::Codex && session.session.id == plan.parent_id())
        else {
            failures.push(format!(
                "Codex deletion plan lost selected parent: {}",
                plan.parent_id()
            ));
            continue;
        };
        if let Err(error) = delete_codex_plan(&plan) {
            failures.push(format!("Codex session {}: {error}", parent.session.id));
        } else {
            println!("{} {parent}", "Deleted".red().bold());
            println!(
                "  └─ {} (subagents: {})",
                parent.session.id.white().bold(),
                plan.descendant_count()
            );
        }
    }
}

fn delete_codex_plan(plan: &ytil_agents::agent::session_loader::codex::DeletionPlan) -> rootcause::Result<()> {
    delete_paths_in_order(plan.paths(), delete_session_path)
}

fn delete_paths_in_order(
    paths: &[std::path::PathBuf],
    delete_path: impl FnMut(&Path) -> rootcause::Result<()>,
) -> rootcause::Result<()> {
    paths.iter().map(std::path::PathBuf::as_path).try_for_each(delete_path)
}

fn delete_session_path(delete_path: &Path) -> rootcause::Result<()> {
    if delete_path.is_dir() {
        std::fs::remove_dir_all(delete_path)
            .context("failed to delete session directory")
            .attach_with(|| format!("path={}", delete_path.display()))?;
    } else {
        std::fs::remove_file(delete_path)
            .context("failed to delete session file")
            .attach_with(|| format!("path={}", delete_path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn delete_paths_in_order_when_a_child_deletion_fails_keeps_its_parent_path() {
        let paths = vec![std::path::PathBuf::from("child"), std::path::PathBuf::from("parent")];
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
        assert_that!(deleted, eq(Vec::<std::path::PathBuf>::new()));
    }

    #[test]
    fn delete_paths_in_order_when_another_tree_succeeds_deletes_its_paths() {
        let failed_tree = vec![
            std::path::PathBuf::from("failed-child"),
            std::path::PathBuf::from("failed-parent"),
        ];
        let successful_tree = vec![
            std::path::PathBuf::from("other-child"),
            std::path::PathBuf::from("other-parent"),
        ];
        let mut deleted = Vec::new();

        let failed_result = delete_paths_in_order(&failed_tree, |_| Err(report!("child deletion failed")));
        let successful_result = delete_paths_in_order(&successful_tree, |path| {
            deleted.push(path.to_path_buf());
            Ok(())
        });

        assert_that!(
            failed_result,
            err(displays_as(contains_substring("child deletion failed")))
        );
        assert_that!(successful_result, ok(eq(())));
        assert_that!(deleted, eq(successful_tree));
    }

    #[test]
    fn delete_session_path_when_given_a_file_removes_only_that_file() {
        let dir = tempdir().expect("tempdir should be created");
        let parent = dir.path().join("parent.jsonl");
        let child = dir.path().join("child.jsonl");
        std::fs::write(&parent, "parent").expect("parent fixture should be written");
        std::fs::write(&child, "child").expect("child fixture should be written");

        let result = delete_session_path(&parent);

        assert_that!(result, ok(eq(())));
        assert_that!(parent.exists(), eq(false));
        assert_that!(child.exists(), eq(true));
    }
}
