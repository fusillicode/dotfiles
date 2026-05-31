use muxr_core::INTERNAL_SERVER_ARG;
use muxr_core::SessionName;
use rootcause::prelude::ResultExt;
use rootcause::report;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Help,
    Start { session: SessionName },
}

impl Command {
    /// Execute the muxr CLI command.
    ///
    /// # Errors
    /// - The home state path cannot be resolved.
    fn execute(self) -> rootcause::Result<()> {
        match self {
            Self::Help => print!("{}", include_str!("../help.txt")),
            Self::Start { session } => {
                let server_executable = std::env::current_exe().context("failed to resolve muxr executable")?;
                muxr_client::start(&session, &server_executable)?;
            }
        }

        Ok(())
    }
}

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();

    if let Some(session) = parse_internal_server(&args)? {
        muxr_server::serve_session(&session)?;
        return Ok(());
    }

    let command = parse(&args);

    match command {
        Ok(command) => command.execute(),
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
/// - The command is unknown, has unexpected extra arguments, or uses an invalid session name.
fn parse(args: &[String]) -> rootcause::Result<Command> {
    if args.iter().any(|arg| arg == "--help") {
        return Ok(Command::Help);
    }

    match args {
        [] => Ok(Command::Help),
        [command] if command == "start" => Ok(Command::Start {
            session: SessionName::default(),
        }),
        [command, session] if command == "start" => Ok(Command::Start {
            session: session.parse()?,
        }),
        [command, session, rest @ ..] if command == "start" => {
            Err(report!("unexpected muxr start args").attach(format!("session={session:?} extra={rest:?}")))
        }
        [command, ..] => Err(report!("unknown muxr command {command:?}")),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::start_without_session(&["start"], "default")]
    #[case::start_with_session(&["start", "work"], "work")]
    fn test_parse_when_start_args_vary_returns_start_session(
        #[case] raw: &[&str],
        #[case] expected_session: &str,
    ) -> rootcause::Result<()> {
        assert2::assert!(let Command::Start { session } = parse(&args(raw))?);
        pretty_assertions::assert_eq!(session.as_ref(), expected_session);
        Ok(())
    }

    #[rstest]
    #[case::empty_args(&[])]
    #[case::help_arg(&["--help"])]
    #[case::help_among_args(&["start", "--help"])]
    fn test_parse_when_help_requested_returns_help(#[case] raw: &[&str]) -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(parse(&args(raw))?, Command::Help);
        Ok(())
    }

    #[rstest]
    #[case::start_extra_args(&["start", "work", "extra"])]
    #[case::old_attach_command(&["attach"])]
    #[case::old_detach_command(&["detach"])]
    #[case::old_server_command(&["server", "work"])]
    #[case::unknown_command(&["bogus"])]
    fn test_parse_when_args_are_invalid_returns_error(#[case] raw: &[&str]) {
        assert2::assert!(parse(&args(raw)).is_err());
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

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(ToString::to_string).collect()
    }
}
