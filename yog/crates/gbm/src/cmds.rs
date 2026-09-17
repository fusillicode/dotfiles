use rootcause::report;
use ytil_sys::cli::Args;

pub mod init;
pub mod install;
pub mod pick;
pub mod rename;

const ZSHRC_INSTALL_LINE: &str = r#"(( $+commands[gbm] )) && eval "$(gbm init zsh)""#;
const ZSH_WRAPPER: &str = r#"gbm() {
  if (( $# == 0 )); then
    local branch
    branch="$(command gbm --pick)" || return
    [[ -n "$branch" ]] || return
    print -z -- "gbm ${(q)branch}"
    return
  fi

  command gbm "$@"
}
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Help {
    Root,
    Install,
    Init,
    Pick,
}

impl Help {
    pub(crate) fn from_args(args: &[String]) -> Self {
        match args.first().map(String::as_str) {
            Some("install") => Self::Install,
            Some("init") => Self::Init,
            Some("--pick") => Self::Pick,
            _ => Self::Root,
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Self::Root => include_str!("../help.txt"),
            Self::Install => include_str!("../help/install/help.txt"),
            Self::Init => include_str!("../help/init/help.txt"),
            Self::Pick => include_str!("../help/pick/help.txt"),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum Cmd {
    Help(Help),
    Pick,
    Install,
    InitZsh,
    Rename(String),
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

        match args.as_slice() {
            [] => Err(report!("gbm shell wrapper is not installed or loaded")
                .attach("run `gbm install` first, then restart zsh or source ~/.zshrc")),
            [argument] if argument == "--pick" => Ok(Self::Pick),
            [argument] if argument == "install" => Ok(Self::Install),
            [argument] if argument == "init" => Err(report!("missing gbm init shell")),
            [first, second] if first == "init" && second == "zsh" => Ok(Self::InitZsh),
            [first, ..] if first == "init" => Err(report!("unsupported gbm init shell")),
            [branch_name] => Ok(Self::Rename(branch_name.clone())),
            _ => Ok(Self::Rename(args.join("-"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[rstest::rstest]
    #[case::help(vec!["--help"], Cmd::Help(Help::Root))]
    #[case::pick(vec!["--pick"], Cmd::Pick)]
    #[case::install(vec!["install"], Cmd::Install)]
    #[case::init_zsh(vec!["init", "zsh"], Cmd::InitZsh)]
    #[case::rename(vec!["feature"], Cmd::Rename("feature".to_owned()))]
    #[case::join_rename_parts(
        vec!["feature", "one"],
        Cmd::Rename("feature-one".to_owned())
    )]
    fn test_parse_known_commands_returns_expected_command(#[case] args: Vec<&str>, #[case] expected: Cmd) {
        assert_that!(parse(args), ok(eq(expected)));
    }

    #[test]
    fn test_parse_without_arguments_returns_wrapper_error() {
        assert_that!(Cmd::try_from(Vec::new()), err(anything()));
    }

    #[rstest::rstest]
    #[case::root(vec!["--help"], Help::Root)]
    #[case::install(vec!["install", "--help"], Help::Install)]
    #[case::init(vec!["init", "--help"], Help::Init)]
    #[case::pick(vec!["--pick", "--help"], Help::Pick)]
    fn test_help_when_command_varies_selects_the_matching_command(#[case] args: Vec<&str>, #[case] expected: Help) {
        let args = args.into_iter().map(ToOwned::to_owned).collect::<Vec<_>>();
        assert_that!(Help::from_args(&args), eq(expected));
    }

    fn parse(args: Vec<&str>) -> rootcause::Result<Cmd> {
        Cmd::try_from(args.into_iter().map(ToOwned::to_owned).collect::<Vec<String>>())
    }
}
