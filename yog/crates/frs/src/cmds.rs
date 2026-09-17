use std::ffi::OsString;

use rootcause::report;
use ytil_sys::pico_args::Arguments;

pub mod repo;
pub mod rsl;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Help {
    Root,
    Repo,
    RepoFix,
    Rsl,
}

impl Help {
    pub(crate) fn from_args(args: &[OsString]) -> Self {
        match args.first().map(|arg| arg.to_string_lossy()).as_deref() {
            Some("repo") if args.get(1).is_some_and(|arg| arg == "fix") => Self::RepoFix,
            Some("repo") => Self::Repo,
            Some("rsl") => Self::Rsl,
            _ => Self::Root,
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Self::Root => include_str!("../help.txt"),
            Self::Repo => include_str!("../help/repo/help.txt"),
            Self::RepoFix => include_str!("../help/repo/fix/help.txt"),
            Self::Rsl => include_str!("../help/rsl/help.txt"),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum Cmd {
    Help(Help),
    Repo(Vec<OsString>),
    Rsl(Vec<OsString>),
}

impl Cmd {
    pub fn from_env() -> rootcause::Result<Self> {
        let args = Arguments::from_env();
        let help = Help::from_args(&args.clone().finish());
        Self::try_from(args).inspect_err(|_| eprintln!("{}", help.text()))
    }
}

impl TryFrom<Arguments> for Cmd {
    type Error = rootcause::Report;

    fn try_from(mut args: Arguments) -> Result<Self, Self::Error> {
        let Some(command) = args.subcommand()? else {
            return if args.contains("--help") || args.finish().is_empty() {
                Ok(Self::Help(Help::Root))
            } else {
                Err(report!("unsupported frs command"))
            };
        };

        if args.clone().contains("--help") {
            let raw_args = args.clone().finish();
            let full_args = std::iter::once(OsString::from(command.as_str()))
                .chain(raw_args)
                .collect::<Vec<_>>();
            return Ok(Self::Help(Help::from_args(&full_args)));
        }

        let remaining = args.finish();
        match command.as_str() {
            "repo" => Ok(Self::Repo(remaining)),
            "rsl" => Ok(Self::Rsl(remaining)),
            command => Err(report!("unsupported frs command").attach(format!("command={command}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use test_that::prelude::*;
    use ytil_sys::pico_args::Arguments;

    use super::*;

    #[rstest::rstest]
    #[case::bare(&[], Cmd::Help(Help::Root))]
    #[case::help(&["--help"], Cmd::Help(Help::Root))]
    #[case::repo(&["repo"], Cmd::Repo(Vec::new()))]
    #[case::repo_fix(
        &["repo", "fix", "--clean"],
        Cmd::Repo(vec![OsString::from("fix"), OsString::from("--clean")])
    )]
    #[case::repo_help(&["repo", "--help"], Cmd::Help(Help::Repo))]
    #[case::repo_fix_help(&["repo", "fix", "--help"], Cmd::Help(Help::RepoFix))]
    #[case::rsl(&["rsl", "--json", "sample.rs"], Cmd::Rsl(vec![
        OsString::from("--json"),
        OsString::from("sample.rs")
    ]))]
    #[case::rsl_help(&["rsl", "--help"], Cmd::Help(Help::Rsl))]
    fn test_parse_known_commands(#[case] args: &[&str], #[case] expected: Cmd) {
        assert_that!(parse(args), ok(eq(expected)));
    }

    #[rstest::rstest]
    #[case::unexpected_argument(&["unexpected"])]
    #[case::unknown_command(&["unknown", "--value"])]
    fn test_parse_rejects_invalid_commands(#[case] args: &[&str]) {
        assert_that!(parse(args), err(anything()));
    }

    fn parse(args: &[&str]) -> rootcause::Result<Cmd> {
        Cmd::try_from(Arguments::from_vec(args.iter().map(OsString::from).collect()))
    }
}
