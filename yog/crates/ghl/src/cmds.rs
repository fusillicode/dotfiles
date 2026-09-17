use std::ffi::OsString;

use ytil_sys::cli::Args;
use ytil_sys::pico_args::Arguments;

pub mod branch;
pub mod issue;
pub mod list;
pub mod pr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Help {
    Root,
    List,
    Issue,
    Pr,
    Branch,
}

impl Help {
    pub(crate) fn from_args(args: &[OsString]) -> Self {
        if has_command(args, "issue") {
            return Self::Issue;
        }
        if has_command(args, "pr") {
            return Self::Pr;
        }
        if has_command(args, "branch") {
            return Self::Branch;
        }
        if args.is_empty() || args.iter().all(|arg| arg == "--help") {
            Self::Root
        } else {
            Self::List
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Self::Root => include_str!("../help.txt"),
            Self::List => include_str!("../help/list/help.txt"),
            Self::Issue => include_str!("../help/issue/help.txt"),
            Self::Pr => include_str!("../help/pr/help.txt"),
            Self::Branch => include_str!("../help/branch/help.txt"),
        }
    }
}

pub enum Cmd {
    Help(Help),
    List(Arguments),
    Issue,
    Pr,
    Branch,
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

    fn try_from(args: Arguments) -> Result<Self, Self::Error> {
        let raw_args = args.clone().finish();
        if args.has_help() {
            return Ok(Self::Help(Help::from_args(&raw_args)));
        }

        if has_command(&raw_args, "issue") {
            return Ok(Self::Issue);
        }

        if has_command(&raw_args, "pr") {
            return Ok(Self::Pr);
        }

        if has_command(&raw_args, "branch") {
            return Ok(Self::Branch);
        }

        Ok(Self::List(args))
    }
}

fn has_command(args: &[OsString], command: &str) -> bool {
    let mut skip_next = false;
    for argument in args {
        if skip_next {
            skip_next = false;
            continue;
        }

        let argument = argument.to_string_lossy();
        if matches!(argument.as_ref(), "--search" | "--merge-state") {
            skip_next = true;
            continue;
        }
        if argument.starts_with("--search=") || argument.starts_with("--merge-state=") {
            continue;
        }
        if argument == command {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use test_that::prelude::*;
    use ytil_sys::pico_args::Arguments;

    use super::*;

    #[test]
    fn test_parse_when_help_is_requested_returns_help() {
        assert!(matches!(parse(&["--help"]), Ok(Cmd::Help(Help::Root))));
    }

    #[rstest::rstest]
    #[case::issue("issue", "issue")]
    #[case::pull_request("pr", "pr")]
    #[case::branch("branch", "branch")]
    fn test_parse_when_named_command_is_supplied_returns_command(#[case] command: &str, #[case] expected: &str) {
        let parsed = parse(&[command]);
        assert!(matches!(
            (expected, parsed),
            ("issue", Ok(Cmd::Issue)) | ("pr", Ok(Cmd::Pr)) | ("branch", Ok(Cmd::Branch))
        ));
    }

    #[test]
    fn test_parse_when_no_named_command_is_supplied_returns_list_command() {
        assert!(matches!(parse(&["--search", "lint"]), Ok(Cmd::List(_))));
    }

    #[test]
    fn test_parse_when_command_name_is_search_value_returns_list_command() {
        assert!(matches!(parse(&["--search", "issue"]), Ok(Cmd::List(_))));
    }

    #[rstest::rstest]
    #[case::root(&["--help"], Help::Root)]
    #[case::list(&["--search", "lint", "--help"], Help::List)]
    #[case::issue(&["issue", "--help"], Help::Issue)]
    #[case::pull_request(&["pr", "--help"], Help::Pr)]
    #[case::branch(&["branch", "--help"], Help::Branch)]
    fn test_help_when_command_varies_selects_the_matching_command(#[case] raw: &[&str], #[case] expected: Help) {
        let args = raw.iter().map(OsString::from).collect::<Vec<_>>();
        assert_that!(Help::from_args(&args), eq(expected));
    }

    fn parse(args: &[&str]) -> rootcause::Result<Cmd> {
        Cmd::try_from(Arguments::from_vec(args.iter().map(OsString::from).collect()))
    }
}
