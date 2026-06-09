use std::fmt;
use std::path::Path;
use std::path::PathBuf;

use muxr_core::EXTERNAL_LAYOUT_ARG;
use muxr_core::SessionName;
use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use strum::EnumIter;
use strum::IntoEnumIterator;

const SERVER_EXECUTABLE: &str = "muxr-server";

#[derive(Clone, Debug, Eq, PartialEq)]
enum Cmd {
    Help,
    Sessions,
    Start {
        session: SessionName,
        external_layout: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq)]
enum SessionAction {
    Attach,
    Delete,
}

impl Cmd {
    /// Parse muxr CLI arguments.
    ///
    /// # Errors
    /// - The cmd is unknown, has unexpected extra arguments, or uses an invalid session name.
    fn parse(args: &[String]) -> rootcause::Result<Self> {
        if args.iter().any(|arg| arg == "--help") {
            return Ok(Self::Help);
        }

        match args {
            [] => Ok(Self::Sessions),
            [cmd, rest @ ..] if cmd == "start" => Self::parse_start(rest),
            [cmd, ..] => Err(report!("unknown muxr cmd {cmd:?}")),
        }
    }

    fn parse_start(args: &[String]) -> rootcause::Result<Self> {
        match args {
            [] => Ok(Self::Start {
                session: SessionName::default(),
                external_layout: None,
            }),
            [layout_flag, layout] if layout_flag == EXTERNAL_LAYOUT_ARG => Ok(Self::Start {
                session: SessionName::default(),
                external_layout: Some(PathBuf::from(layout)),
            }),
            [layout_flag] if layout_flag == EXTERNAL_LAYOUT_ARG => {
                Err(report!("missing muxr start layout").attach(format!("flag={EXTERNAL_LAYOUT_ARG}")))
            }
            [session] => Ok(Self::Start {
                session: session.parse()?,
                external_layout: None,
            }),
            [session, layout_flag, layout] if layout_flag == EXTERNAL_LAYOUT_ARG => Ok(Self::Start {
                session: session.parse()?,
                external_layout: Some(PathBuf::from(layout)),
            }),
            [session, layout_flag] if layout_flag == EXTERNAL_LAYOUT_ARG => {
                Err(report!("missing muxr start layout").attach(format!("session={session:?}")))
            }
            _ => Err(report!("unexpected muxr start args").attach(format!("args={args:?}"))),
        }
    }

    /// Execute the muxr CLI cmd.
    ///
    /// # Errors
    /// - The home state path cannot be resolved.
    fn execute(self) -> rootcause::Result<()> {
        match self {
            Self::Help => print!("{}", include_str!("../help.txt")),
            Self::Sessions => self::run_session_picker()?,
            Self::Start {
                session,
                external_layout,
            } => {
                let current_exe = std::env::current_exe().context("failed to resolve muxr executable")?;
                let server_executable = self::server_executable_next_to(&current_exe)?;
                let external_layout = match external_layout {
                    Some(path) if path.is_relative() => Some(
                        std::env::current_dir()
                            .context("failed to resolve muxr cwd")?
                            .join(path),
                    ),
                    Some(path) => Some(path),
                    None => None,
                };
                muxr_client::start(&session, &server_executable, external_layout.as_deref())?;
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
    let cmd = Cmd::parse(&args);

    match cmd {
        Ok(cmd) => cmd.execute(),
        Err(err) => {
            print!("{}", include_str!("../help.txt"));
            Err(err)
        }
    }
}

fn run_session_picker() -> rootcause::Result<()> {
    let sessions = muxr_client::list_sessions()?;
    if sessions.is_empty() {
        let current_exe = std::env::current_exe().context("failed to resolve muxr executable")?;
        let server_executable = self::server_executable_next_to(&current_exe)?;
        muxr_client::start(&SessionName::default(), &server_executable, None)?;
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
            let current_exe = std::env::current_exe().context("failed to resolve muxr executable")?;
            let server_executable = self::server_executable_next_to(&current_exe)?;
            muxr_client::start(session, &server_executable, None)
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

fn server_executable_next_to(current_exe: &Path) -> rootcause::Result<PathBuf> {
    let Some(parent) = current_exe.parent().filter(|parent| !parent.as_os_str().is_empty()) else {
        return Err(
            report!("muxr executable has no parent dir").attach(format!("executable={}", current_exe.display()))
        );
    };
    // Keep the attached client and long-lived server as separate processes: `muxr` can link picker/UI-only CLI deps,
    // while `muxr-server` keeps session state, PTYs, and scrollback memory attributable to the server runtime alone.
    Ok(parent.join(SERVER_EXECUTABLE))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::start_without_session(&["start"], "default", None)]
    #[case::start_with_session(&["start", "work"], "work", None)]
    #[case::start_default_with_layout(
        &["start", "--layout", "../.config/muxr/layouts/work.json"],
        "default",
        Some("../.config/muxr/layouts/work.json")
    )]
    #[case::start_session_with_layout(
        &["start", "work", "--layout", ".config/muxr/layouts/work.json"],
        "work",
        Some(".config/muxr/layouts/work.json")
    )]
    fn test_parse_when_start_args_vary_returns_start_cmd(
        #[case] raw: &[&str],
        #[case] expected_session: &str,
        #[case] expected_layout: Option<&str>,
    ) -> rootcause::Result<()> {
        assert2::assert!(let Cmd::Start {
            session,
            external_layout,
        } = Cmd::parse(&args(raw))?);
        pretty_assertions::assert_eq!(session.as_ref(), expected_session);
        pretty_assertions::assert_eq!(external_layout.as_deref().and_then(Path::to_str), expected_layout);
        Ok(())
    }

    #[rstest]
    #[case::help_arg(&["--help"])]
    #[case::help_among_args(&["start", "--help"])]
    fn test_parse_when_help_requested_returns_help(#[case] raw: &[&str]) -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(Cmd::parse(&args(raw))?, Cmd::Help);
        Ok(())
    }

    #[test]
    fn test_parse_when_no_args_returns_session_picker() -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(Cmd::parse(&args(&[]))?, Cmd::Sessions);
        Ok(())
    }

    #[test]
    fn test_server_executable_next_to_returns_sibling_without_checking_existence() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let muxr = tempdir.path().join("muxr");
        let runner = tempdir.path().join(SERVER_EXECUTABLE);

        pretty_assertions::assert_eq!(server_executable_next_to(&muxr)?, runner);
        Ok(())
    }

    #[rstest]
    #[case::start_extra_args(&["start", "work", "extra"])]
    #[case::start_missing_layout(&["start", "--layout"])]
    #[case::start_session_missing_layout(&["start", "work", "--layout"])]
    #[case::start_layout_extra_args(&["start", "work", "--layout", "work", "extra"])]
    #[case::old_memory_cmd(&["memory"])]
    #[case::unknown_start_flag(&["start", "--bogus"])]
    #[case::old_attach_cmd(&["attach"])]
    #[case::old_detach_cmd(&["detach"])]
    #[case::old_server_cmd(&["server", "work"])]
    #[case::unknown_cmd(&["bogus"])]
    fn test_parse_when_args_are_invalid_returns_error(#[case] raw: &[&str]) {
        assert2::assert!(Cmd::parse(&args(raw)).is_err());
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
    fn test_execute_session_action_when_attach_has_multiple_sessions_returns_error() -> rootcause::Result<()> {
        let sessions = vec![listed_session("work")?, listed_session("notes")?];

        let error = execute_session_action(SessionAction::Attach, &sessions)
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
