use std::path::Path;

use owo_colors::OwoColorize;
use rootcause::report;
use ytil_agents::agent::session::SessionKey;
use ytil_agents::agent::session_deletion::DeletionOutcome;
use ytil_agents::agent::session_deletion::DeletionTarget;

use super::list::RenderableSession;

pub(super) fn delete_selected_sessions(selected: &[RenderableSession], home_dir: &Path) -> rootcause::Result<()> {
    let targets = selected
        .iter()
        .map(|session| {
            DeletionTarget::new(
                SessionKey::new(session.session.agent, &session.session.id),
                session.session.path.clone(),
            )
        })
        .collect::<Vec<_>>();
    let report = ytil_agents::agent::session_deletion::delete_session_targets(home_dir, &targets);
    let mut failures = Vec::new();

    // Outcomes are 1:1 with `selected` / `targets` order; pair by index so duplicate
    // Cursor ids still confirm the row whose path was deleted.
    for (index, outcome) in report.outcomes().iter().enumerate() {
        match outcome {
            DeletionOutcome::Deleted {
                key,
                related_session_count,
            } => {
                if let Some(session) = selected.get(index) {
                    render_deleted_session(session, key, *related_session_count);
                }
            }
            DeletionOutcome::Failed { key, error } => failures.push(format!("{key}: {error}")),
        }
    }
    for skipped_path in report.skipped_paths() {
        println!(
            "{} {}",
            "Skipped unreadable session file".yellow().bold(),
            skipped_path.display()
        );
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(report!("session deletion failed").attach(failures.join("\n")))
    }
}

fn render_deleted_session(session: &RenderableSession, key: &SessionKey, related_session_count: usize) {
    println!("{} {session}", "Deleted".red().bold());
    if related_session_count > 0 {
        println!(
            "  └─ {} (related sessions: {related_session_count})",
            key.id().white().bold()
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use jiff::Timestamp;
    use test_that::prelude::*;
    use ytil_agents::agent::Agent;
    use ytil_agents::agent::session::Session;

    use super::super::list::RenderableSession;

    #[test]
    fn test_session_for_deletion_outcome_when_cursor_ids_duplicate_uses_outcome_index_path() {
        let created_at = Timestamp::from_millisecond(1).expect("test timestamp should be valid");
        let session_id = "24ba5086-7cca-419c-85c7-e9d636670fbe";
        let first_path = PathBuf::from("/tmp/chats/hash-a").join(session_id);
        let second_path = PathBuf::from("/tmp/chats/hash-b").join(session_id);
        let home_dir = PathBuf::from("/tmp");
        let selected = [
            RenderableSession::for_test(
                Session {
                    id: session_id.to_owned(),
                    agent: Agent::Cursor,
                    name: "Code Review Comments".to_owned(),
                    last_user_prompt: None,
                    search_text: "Code Review Comments".to_owned(),
                    workspace: PathBuf::from("/"),
                    path: first_path.clone(),
                    created_at,
                    updated_at: created_at,
                },
                home_dir.clone(),
            ),
            RenderableSession::for_test(
                Session {
                    id: session_id.to_owned(),
                    agent: Agent::Cursor,
                    name: "KPay B2B Denormalized".to_owned(),
                    last_user_prompt: None,
                    search_text: "KPay B2B Denormalized".to_owned(),
                    workspace: PathBuf::from("/tmp/conversions"),
                    path: second_path.clone(),
                    created_at,
                    updated_at: created_at,
                },
                home_dir,
            ),
        ];

        let first = selected.first().expect("first outcome should resolve");
        let second = selected.get(1).expect("second outcome should resolve");

        assert_that!(first.session.path, eq(first_path));
        assert_that!(second.session.path, eq(second_path));
        assert_that!(first.session.id.as_str(), eq(second.session.id.as_str()));
    }
}
