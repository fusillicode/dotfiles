use rootcause::report;
use ytil_sys::cli::Args;

pub mod ci;
pub mod local;

#[derive(Debug, Eq, PartialEq)]
pub enum Cmd {
    Help,
    Ci(ci::CmdKind),
    Local(Vec<String>),
}

impl Cmd {
    pub fn from_env() -> rootcause::Result<Self> {
        Self::try_from(ytil_sys::cli::get())
    }
}

impl TryFrom<Vec<String>> for Cmd {
    type Error = rootcause::Report;

    fn try_from(args: Vec<String>) -> Result<Self, Self::Error> {
        if args.has_help() {
            return Ok(Self::Help);
        }

        if args.first().is_some_and(|arg| arg == "ci") {
            let command = ci::cmd_from_args(&args)?.ok_or_else(|| report!("missing evoke ci command"))?;
            return Ok(Self::Ci(command));
        }

        Ok(Self::Local(args))
    }
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[rstest::rstest]
    #[case::help(vec!["--help"], Cmd::Help)]
    #[case::local(vec!["--debug"], Cmd::Local(vec!["--debug".to_owned()]))]
    #[case::ci_default(vec!["ci"], Cmd::Ci(ci::CmdKind::All))]
    #[case::ci_lint(vec!["ci", "lint"], Cmd::Ci(ci::CmdKind::Lint))]
    fn test_parse_known_commands_returns_expected_command(#[case] args: Vec<&str>, #[case] expected: Cmd) {
        assert_that!(parse(args), ok(eq(expected)));
    }

    #[rstest::rstest]
    #[case::ci_extra_arg(vec!["ci", "lint", "extra"])]
    #[case::ci_unknown_command(vec!["ci", "unknown"])]
    fn test_parse_invalid_ci_commands_returns_error(#[case] args: Vec<&str>) {
        assert_that!(parse(args), err(anything()));
    }

    fn parse(args: Vec<&str>) -> rootcause::Result<Cmd> {
        Cmd::try_from(args.into_iter().map(ToOwned::to_owned).collect::<Vec<String>>())
    }
}
