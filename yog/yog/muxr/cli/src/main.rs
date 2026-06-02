use std::fmt;
use std::path::Path;

use muxr_core::INTERNAL_SERVER_ARG;
use muxr_core::SessionName;
use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use strum::EnumIter;
use strum::IntoEnumIterator;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Cmd {
    Help,
    Sessions,
    Start { session: SessionName },
}

#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq)]
enum SessionAction {
    Attach,
    Delete,
}

impl Cmd {
    /// Execute the muxr CLI cmd.
    ///
    /// # Errors
    /// - The home state path cannot be resolved.
    fn execute(self) -> rootcause::Result<()> {
        match self {
            Self::Help => print!("{}", include_str!("../help.txt")),
            Self::Sessions => self::run_session_picker()?,
            Self::Start { session } => {
                let server_executable = std::env::current_exe().context("failed to resolve muxr executable")?;
                muxr_client::start(&session, &server_executable)?;
            }
        }

        Ok(())
    }
}

impl fmt::Display for SessionAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Attach => write!(f, "{}", "Attach".green().bold()),
            Self::Delete => write!(f, "{}", "Delete".red().bold()),
        }
    }
}

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();

    if let Some(session) = parse_internal_server(&args)? {
        muxr_server::serve_session(&session)?;
        return Ok(());
    }

    let cmd = parse(&args);

    match cmd {
        Ok(cmd) => cmd.execute(),
        Err(err) => {
            print!("{}", include_str!("../help.txt"));
            Err(err)
        }
    }
}

/// Parse hidden internal server arguments.
///
/// # Errors
/// - The internal server invocation is missing its session name, has extra args, or has an invalid session name.
fn parse_internal_server(args: &[String]) -> rootcause::Result<Option<SessionName>> {
    match args {
        [flag, session] if flag == INTERNAL_SERVER_ARG => Ok(Some(session.parse()?)),
        [flag, rest @ ..] if flag == INTERNAL_SERVER_ARG => {
            Err(report!("unexpected muxr internal server args").attach(format!("args={rest:?}")))
        }
        _ => Ok(None),
    }
}

/// Parse muxr CLI arguments.
///
/// # Errors
/// - The cmd is unknown, has unexpected extra arguments, or uses an invalid session name.
fn parse(args: &[String]) -> rootcause::Result<Cmd> {
    if args.iter().any(|arg| arg == "--help") {
        return Ok(Cmd::Help);
    }

    match args {
        [] => Ok(Cmd::Sessions),
        [cmd] if cmd == "start" => Ok(Cmd::Start {
            session: SessionName::default(),
        }),
        [cmd, session] if cmd == "start" => Ok(Cmd::Start {
            session: session.parse()?,
        }),
        [cmd, session, rest @ ..] if cmd == "start" => {
            Err(report!("unexpected muxr start args").attach(format!("session={session:?} extra={rest:?}")))
        }
        [cmd, ..] => Err(report!("unknown muxr cmd {cmd:?}")),
    }
}

fn run_session_picker() -> rootcause::Result<()> {
    let server_executable = std::env::current_exe().context("failed to resolve muxr executable")?;
    let sessions = muxr_client::list_sessions()?;
    if sessions.is_empty() {
        muxr_client::start(&SessionName::default(), &server_executable)?;
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
    self::execute_session_action(action, &sessions, &server_executable)
}

fn execute_session_action(
    action: SessionAction,
    selected: &[SessionName],
    server_executable: &Path,
) -> rootcause::Result<()> {
    match action {
        SessionAction::Attach => {
            let session = ytil_tui::require_single(selected, "sessions")?;
            muxr_client::start(session, server_executable)
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

    use super::*;

    #[rstest]
    #[case::start_without_session(&["start"], "default")]
    #[case::start_with_session(&["start", "work"], "work")]
    fn test_parse_when_start_args_vary_returns_start_cmd(
        #[case] raw: &[&str],
        #[case] expected_session: &str,
    ) -> rootcause::Result<()> {
        assert2::assert!(let Cmd::Start { session } = parse(&args(raw))?);
        pretty_assertions::assert_eq!(session.as_ref(), expected_session);
        Ok(())
    }

    #[rstest]
    #[case::help_arg(&["--help"])]
    #[case::help_among_args(&["start", "--help"])]
    fn test_parse_when_help_requested_returns_help(#[case] raw: &[&str]) -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(parse(&args(raw))?, Cmd::Help);
        Ok(())
    }

    #[test]
    fn test_parse_when_no_args_returns_session_picker() -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(parse(&args(&[]))?, Cmd::Sessions);
        Ok(())
    }

    #[rstest]
    #[case::start_extra_args(&["start", "work", "extra"])]
    #[case::old_attach_cmd(&["attach"])]
    #[case::old_detach_cmd(&["detach"])]
    #[case::old_server_cmd(&["server", "work"])]
    #[case::unknown_cmd(&["bogus"])]
    fn test_parse_when_args_are_invalid_returns_error(#[case] raw: &[&str]) {
        assert2::assert!(parse(&args(raw)).is_err());
    }

    #[rstest]
    #[case::attach(SessionAction::Attach, format!("{}", "Attach".green().bold()))]
    #[case::delete(SessionAction::Delete, format!("{}", "Delete".red().bold()))]
    fn test_session_action_display_when_action_varies_matches_zj_style(
        #[case] action: SessionAction,
        #[case] expected: String,
    ) {
        pretty_assertions::assert_eq!(action.to_string(), expected);
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

        assert2::assert!(result.is_err());
        pretty_assertions::assert_eq!(attempted, vec!["ok", "bad", "later"]);
        Ok(())
    }

    #[test]
    fn test_parse_internal_server_when_server_arg_has_session_returns_session() -> rootcause::Result<()> {
        let Some(session) = parse_internal_server(&args(&["--server", "work"]))? else {
            return Err(report!("expected internal server session"));
        };

        pretty_assertions::assert_eq!(session.as_ref(), "work");
        Ok(())
    }

    #[rstest]
    #[case::missing_session(&["--server"])]
    #[case::extra_args(&["--server", "work", "extra"])]
    fn test_parse_internal_server_when_args_are_invalid_returns_error(#[case] raw: &[&str]) {
        assert2::assert!(parse_internal_server(&args(raw)).is_err());
    }

    #[test]
    fn test_execute_session_action_when_attach_has_multiple_sessions_returns_error() -> rootcause::Result<()> {
        let sessions = vec![listed_session("work")?, listed_session("notes")?];

        let error = execute_session_action(SessionAction::Attach, &sessions, Path::new("/muxr"))
            .expect_err("expected attach multi-selection error");

        assert2::assert!(error.to_string().contains("expected exactly one selection"));
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
        pretty_assertions::assert_eq!(delete_session_message(&listed_session("work")?, outcome), expected);
        Ok(())
    }

    #[test]
    fn test_delete_session_failure_message_colors_failure_prefix() -> rootcause::Result<()> {
        let error = report!("delete failed");
        let message = delete_session_failure_message(&listed_session("work")?, &error);

        assert2::assert!(message.starts_with(&format!("{} to delete session work:", "Failed".red().bold())));
        assert2::assert!(message.contains("delete failed"));
        Ok(())
    }

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(ToString::to_string).collect()
    }

    fn listed_session(raw: &str) -> rootcause::Result<SessionName> {
        raw.parse()
    }
}
