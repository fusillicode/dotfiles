//! Session management commands for `muxr`.

use std::fmt;

use muxr_core::SessionName;
use owo_colors::OwoColorize;
use rootcause::report;
use strum::EnumIter;
use strum::IntoEnumIterator;

#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq)]
enum SessionAction {
    Attach,
    Delete,
}

impl fmt::Display for SessionAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Attach => write!(f, "{}", "Attach".green().bold()),
            Self::Delete => write!(f, "{}", "Delete".red().bold()),
        }
    }
}

pub fn run() -> rootcause::Result<()> {
    let sessions = muxr_client::list_sessions()?;
    if sessions.is_empty() {
        super::start::run(&SessionName::default(), None)?;
        return Ok(());
    }

    let Some(selected) = ytil_tui::minimal_multi_select(
        sessions,
        muxr_client::ListedSession::display_text,
        muxr_client::ListedSession::search_text,
    )?
    else {
        println!("No sessions selected");
        return Ok(());
    };

    let Some(action) = ytil_tui::minimal_select::<SessionAction>(SessionAction::iter().collect())? else {
        println!("No action selected");
        return Ok(());
    };

    let sessions = selected
        .iter()
        .map(|session| session.name().clone())
        .collect::<Vec<_>>();
    self::execute_session_action(action, &sessions)
}

fn execute_session_action(action: SessionAction, selected: &[SessionName]) -> rootcause::Result<()> {
    match action {
        SessionAction::Attach => {
            let session = ytil_tui::require_single(selected, "sessions")?;
            super::start::run(session, None)
        }
        SessionAction::Delete => self::delete_selected_sessions(selected, muxr_client::delete_session),
    }
}

fn delete_selected_sessions<F>(selected: &[SessionName], mut delete_session: F) -> rootcause::Result<()>
where
    F: FnMut(&SessionName) -> rootcause::Result<muxr_client::SessionDeleteOutcome>,
{
    let mut failures = Vec::new();

    for session in selected {
        match delete_session(session) {
            Ok(outcome) => println!("{}", self::delete_session_message(session, outcome)),
            Err(error) => {
                // Batch delete must attempt every selected session so one corrupt entry cannot block cleanup.
                eprintln!("{}", self::delete_session_failure_message(session, &error));
                failures.push(format!("{session}: {error}"));
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(report!("failed to delete selected muxr sessions").attach(failures.join("\n")))
    }
}

fn delete_session_message(session: &SessionName, outcome: muxr_client::SessionDeleteOutcome) -> String {
    let deleted = format!("{}", "Deleted".red().bold());
    match outcome {
        muxr_client::SessionDeleteOutcome::LiveDeleted => {
            format!("{deleted} {session}; stopped live server and removed state")
        }
        muxr_client::SessionDeleteOutcome::LiveVanishedForced => {
            format!("{deleted} {session}; live server vanished, force-removed selected session files")
        }
        muxr_client::SessionDeleteOutcome::StoppedRemoved => {
            format!("{deleted} {session}; removed stopped session state")
        }
        muxr_client::SessionDeleteOutcome::UnknownForced => {
            format!("{deleted} {session}; force-removed unknown session files")
        }
    }
}

fn delete_session_failure_message(session: &SessionName, error: impl fmt::Display) -> String {
    let failed = format!("{}", "Failed".red().bold());
    format!("{failed} to delete session {session}: {error}")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use test_that::prelude::*;

    use super::*;

    #[rstest]
    #[case::attach(SessionAction::Attach, format!("{}", "Attach".green().bold()))]
    #[case::delete(SessionAction::Delete, format!("{}", "Delete".red().bold()))]
    fn test_session_action_display_when_action_varies_matches_zj_style(
        #[case] action: SessionAction,
        #[case] expected: String,
    ) {
        assert_that!(action.to_string(), eq(expected));
    }

    #[test]
    fn test_delete_selected_sessions_when_one_delete_fails_still_attempts_all() -> rootcause::Result<()> {
        let selected = ["ok", "bad", "later"]
            .into_iter()
            .map(str::parse)
            .collect::<rootcause::Result<Vec<SessionName>>>()?;
        let mut attempted = Vec::new();

        let result = delete_selected_sessions(&selected, |session| {
            attempted.push(session.to_string());
            if session.as_ref() == "bad" {
                Err(report!("delete failed"))
            } else {
                Ok(muxr_client::SessionDeleteOutcome::StoppedRemoved)
            }
        });

        assert_that!(result, err(anything()));
        assert_that!(attempted, eq(vec!["ok", "bad", "later"]));
        Ok(())
    }

    #[test]
    fn test_execute_session_action_when_attach_has_multiple_sessions_returns_error() -> rootcause::Result<()> {
        let sessions = vec![listed_session("work")?, listed_session("notes")?];

        let error = execute_session_action(SessionAction::Attach, &sessions)
            .expect_err("expected attach multi-selection error");

        assert_that!(error.to_string(), contains_substring("expected exactly one selection"));
        Ok(())
    }

    #[rstest]
    #[case::live_deleted(
        muxr_client::SessionDeleteOutcome::LiveDeleted,
        format!("{} work; stopped live server and removed state", "Deleted".red().bold())
    )]
    #[case::live_vanished_forced(
        muxr_client::SessionDeleteOutcome::LiveVanishedForced,
        format!("{} work; live server vanished, force-removed selected session files", "Deleted".red().bold())
    )]
    #[case::stopped_removed(
        muxr_client::SessionDeleteOutcome::StoppedRemoved,
        format!("{} work; removed stopped session state", "Deleted".red().bold())
    )]
    #[case::unknown_forced(
        muxr_client::SessionDeleteOutcome::UnknownForced,
        format!("{} work; force-removed unknown session files", "Deleted".red().bold())
    )]
    fn test_delete_session_message_when_outcome_varies_reports_behavior(
        #[case] outcome: muxr_client::SessionDeleteOutcome,
        #[case] expected: String,
    ) -> rootcause::Result<()> {
        assert_that!(delete_session_message(&listed_session("work")?, outcome), eq(expected));
        Ok(())
    }

    #[test]
    fn test_delete_session_failure_message_colors_failure_prefix() -> rootcause::Result<()> {
        let error = report!("delete failed");
        let message = delete_session_failure_message(&listed_session("work")?, &error);

        assert_that!(
            message,
            starts_with(format!("{} to delete session work:", "Failed".red().bold()))
        );
        assert_that!(message, contains_substring("delete failed"));
        Ok(())
    }

    fn listed_session(raw: &str) -> rootcause::Result<SessionName> {
        raw.parse()
    }
}
