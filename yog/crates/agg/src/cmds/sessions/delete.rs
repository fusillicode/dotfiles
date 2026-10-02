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
            Ok(DeletionTarget::new(
                SessionKey::new(session.session.agent, &session.session.id)?,
                session.session.path.clone(),
            ))
        })
        .collect::<rootcause::Result<Vec<_>>>()?;
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
                    println!("{}", deleted_session_output(session, key, *related_session_count));
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

fn deleted_session_output(session: &RenderableSession, key: &SessionKey, related_session_count: usize) -> String {
    let related_sessions = if related_session_count > 0 {
        format!(" ({related_session_count} related sessions)")
    } else {
        String::new()
    };
    format!(
        "{} {session}\n  └─ {}{related_sessions}",
        "Deleted".red().bold(),
        key.id().white().bold()
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use jiff::Timestamp;
    use tempfile::tempdir;
    use test_that::prelude::*;
    use ytil_agents::agent::Agent;
    use ytil_agents::agent::session::Session;
    use ytil_agents::agent::session::SessionKey;

    use super::super::list::RenderableSession;
    use super::*;

    #[test]
    fn test_delete_selected_sessions_when_id_is_empty_returns_error_before_deletion() {
        let directory = tempdir().expect("test directory should be created");
        let mut session = render_test_session();
        session.session.id.clear();
        session.session.path = directory.path().join("session.jsonl");
        std::fs::write(&session.session.path, "session content").expect("test session should be written");
        let session_path = session.session.path.clone();

        let error = delete_selected_sessions(&[session], directory.path()).expect_err("empty ID must be rejected");

        assert_eq!(error.format_current_context().to_string(), "invalid session id");
        assert_eq!(
            std::fs::read_to_string(session_path).expect("session must remain readable"),
            "session content"
        );
    }

    #[test]
    fn test_render_deleted_session_when_no_related_sessions_prints_id_without_count() {
        let session = render_test_session();
        let key = SessionKey::new(Agent::Claude, "session-id").expect("test value should be valid");

        let output = rendered_output(&session, &key, 0);

        assert_that!(
            output,
            eq("Deleted cl /workspace 01/01/1970-00:00 01/01/1970-00:00\n  └─ session-id")
        );
    }

    #[test]
    fn test_render_deleted_session_when_related_sessions_exist_prints_id_and_count() {
        let session = render_test_session();
        let key = SessionKey::new(Agent::Claude, "session-id").expect("test value should be valid");

        let output = rendered_output(&session, &key, 2);

        assert_that!(
            output,
            eq("Deleted cl /workspace 01/01/1970-00:00 01/01/1970-00:00\n  └─ session-id (2 related sessions)")
        );
    }

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

    fn rendered_output(session: &RenderableSession, key: &SessionKey, related_session_count: usize) -> String {
        strip_ansi(&deleted_session_output(session, key, related_session_count))
    }

    fn strip_ansi(output: &str) -> String {
        let mut plain = String::new();
        let mut chars = output.chars();
        while let Some(character) = chars.next() {
            if character != '\x1b' {
                plain.push(character);
                continue;
            }
            for escaped in chars.by_ref() {
                if escaped.is_ascii_alphabetic() {
                    break;
                }
            }
        }
        plain
    }

    fn render_test_session() -> RenderableSession {
        let created_at = Timestamp::from_millisecond(1).expect("test timestamp should be valid");
        RenderableSession::for_test(
            Session {
                id: "session-id".to_owned(),
                agent: Agent::Claude,
                name: "session-name".to_owned(),
                last_user_prompt: None,
                search_text: "session-name".to_owned(),
                workspace: PathBuf::from("/workspace"),
                path: PathBuf::from("/sessions/session.jsonl"),
                created_at,
                updated_at: created_at,
            },
            PathBuf::from("/"),
        )
    }
}
