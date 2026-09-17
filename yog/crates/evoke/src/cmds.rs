use rootcause::report;
use ytil_sys::cli::Args;

pub mod ci;
pub mod local;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Help {
    Root,
    Ci,
    CiAll,
    CiLint,
    CiTest,
    CiReleaseNative,
    CiAudit,
}

impl Help {
    pub(crate) fn from_args(args: &[String]) -> Self {
        if args.first().is_none_or(|arg| arg != "ci") {
            return Self::Root;
        }

        match args.get(1).map(String::as_str) {
            Some("all") => Self::CiAll,
            Some("lint") => Self::CiLint,
            Some("test") => Self::CiTest,
            Some("release-native") => Self::CiReleaseNative,
            Some("audit") => Self::CiAudit,
            _ => Self::Ci,
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Self::Root => include_str!("../help.txt"),
            Self::Ci => include_str!("../help/ci/help.txt"),
            Self::CiAll => include_str!("../help/ci/all/help.txt"),
            Self::CiLint => include_str!("../help/ci/lint/help.txt"),
            Self::CiTest => include_str!("../help/ci/test/help.txt"),
            Self::CiReleaseNative => include_str!("../help/ci/release-native/help.txt"),
            Self::CiAudit => include_str!("../help/ci/audit/help.txt"),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum Cmd {
    Help(Help),
    Ci(ci::CmdKind),
    Local(Vec<String>),
}

impl Cmd {
    pub fn from_env() -> rootcause::Result<Self> {
        let args = ytil_sys::cli::get();
        let help = Help::from_args(&args);
        Self::try_from(args).inspect_err(|_| eprintln!("{}", help.text()))
    }
}

impl TryFrom<Vec<String>> for Cmd {
    type Error = rootcause::Report;

    fn try_from(args: Vec<String>) -> Result<Self, Self::Error> {
        if args.has_help() {
            return Ok(Self::Help(Help::from_args(&args)));
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
    #[case::help(vec!["--help"], Cmd::Help(Help::Root))]
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

    #[rstest::rstest]
    #[case::root(vec!["--help"], Help::Root)]
    #[case::ci(vec!["ci", "--help"], Help::Ci)]
    #[case::ci_lint(vec!["ci", "lint", "--help"], Help::CiLint)]
    fn test_help_when_command_path_varies_selects_the_deepest_command(#[case] args: Vec<&str>, #[case] expected: Help) {
        let args = args.into_iter().map(ToOwned::to_owned).collect::<Vec<_>>();
        assert_that!(Help::from_args(&args), eq(expected));
    }

    fn parse(args: Vec<&str>) -> rootcause::Result<Cmd> {
        Cmd::try_from(args.into_iter().map(ToOwned::to_owned).collect::<Vec<String>>())
    }
}
