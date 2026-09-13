use std::ffi::OsString;

use ytil_sys::cli::Args;
use ytil_sys::pico_args::Arguments;

pub mod branch;
pub mod issue;
pub mod list;
pub mod pr;

pub enum Cmd {
    Help,
    List(Arguments),
    Issue,
    Pr,
    Branch,
}

impl Cmd {
    pub fn from_env() -> rootcause::Result<Self> {
        Self::try_from(Arguments::from_env())
    }
}

impl TryFrom<Arguments> for Cmd {
    type Error = rootcause::Report;

    fn try_from(args: Arguments) -> Result<Self, Self::Error> {
        if args.has_help() {
            return Ok(Self::Help);
        }

        let raw_args = args.clone().finish();
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

    use ytil_sys::pico_args::Arguments;

    use super::*;

    #[test]
    fn test_parse_when_help_is_requested_returns_help() {
        assert!(matches!(parse(&["--help"]), Ok(Cmd::Help)));
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

    fn parse(args: &[&str]) -> rootcause::Result<Cmd> {
        Cmd::try_from(Arguments::from_vec(args.iter().map(OsString::from).collect()))
    }
}
