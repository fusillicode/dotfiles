use std::ffi::OsString;

use rootcause::report;
use ytil_sys::pico_args::Arguments;

pub mod repo;
pub mod rsl;

#[derive(Debug, Eq, PartialEq)]
pub enum Cmd {
    Help,
    Repo(Vec<OsString>),
    Rsl(Vec<OsString>),
}

impl Cmd {
    pub fn from_env() -> rootcause::Result<Self> {
        Self::try_from(Arguments::from_env())
    }
}

impl TryFrom<Arguments> for Cmd {
    type Error = rootcause::Report;

    fn try_from(mut args: Arguments) -> Result<Self, Self::Error> {
        let Some(command) = args.subcommand()? else {
            return if args.contains("--help") || args.finish().is_empty() {
                Ok(Self::Help)
            } else {
                Err(report!("unsupported frs command"))
            };
        };

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
    #[case::bare(&[], Cmd::Help)]
    #[case::help(&["--help"], Cmd::Help)]
    #[case::repo(&["repo"], Cmd::Repo(Vec::new()))]
    #[case::repo_fix(
        &["repo", "fix", "--clean"],
        Cmd::Repo(vec![OsString::from("fix"), OsString::from("--clean")])
    )]
    #[case::repo_fix_help(
        &["repo", "fix", "--help"],
        Cmd::Repo(vec![OsString::from("fix"), OsString::from("--help")])
    )]
    #[case::rsl(&["rsl", "--json", "sample.rs"], Cmd::Rsl(vec![
        OsString::from("--json"),
        OsString::from("sample.rs")
    ]))]
    #[case::rsl_help(&["rsl", "--help"], Cmd::Rsl(vec![OsString::from("--help")]))]
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
