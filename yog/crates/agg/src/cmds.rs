use std::ffi::OsString;

use rootcause::Report;
use rootcause::report;
use ytil_sys::pico_args::Arguments;

use crate::cmds::tok::Opts;

pub mod codex;
pub mod sessions;
pub mod sync_harness;
pub mod tok;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Help {
    Root,
    Sessions,
    SessionsList,
    Codex,
    SyncHarness,
    Tok,
}

impl Help {
    pub fn from_args(args: &[OsString]) -> Self {
        match args.first().map(|arg| arg.to_string_lossy()).as_deref() {
            Some("sessions") if args.get(1).is_some_and(|arg| arg == "list") => Self::SessionsList,
            Some("sessions") => Self::Sessions,
            Some("codex") => Self::Codex,
            Some("sync-harness") => Self::SyncHarness,
            Some("tok") => Self::Tok,
            _ => Self::Root,
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Self::Root => include_str!("../help.txt"),
            Self::Sessions => include_str!("../help/sessions/help.txt"),
            Self::SessionsList => include_str!("../help/sessions/list/help.txt"),
            Self::Codex => include_str!("../help/codex/help.txt"),
            Self::SyncHarness => include_str!("../help/sync-harness/help.txt"),
            Self::Tok => include_str!("../help/tok/help.txt"),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum Cmd {
    Help(Help),
    SessionsList,
    SessionsListJson(Vec<String>),
    CodexCompact,
    SyncHarness(sync_harness::Opts),
    Tok(Opts),
}

impl Cmd {
    pub fn from_env() -> rootcause::Result<Self> {
        let args = Arguments::from_env();
        let help = Help::from_args(&args.clone().finish());
        Self::try_from(args).inspect_err(|_| eprintln!("{}", help.text()))
    }

    fn parse_sessions(mut args: Arguments) -> rootcause::Result<Self> {
        match args.subcommand()?.as_deref() {
            Some("list") if args.contains("--json") => Ok(Self::SessionsListJson(strings(args.finish()))),
            Some("list") if args.finish().is_empty() => Ok(Self::SessionsList),
            Some("list") => Err(report!("unsupported agg sessions list command")),
            Some(_) => Err(report!("unsupported agg sessions command")),
            None => Err(report!("missing agg sessions command")),
        }
    }

    fn parse_codex(mut args: Arguments) -> rootcause::Result<Self> {
        if args.contains("--compact") && args.finish().is_empty() {
            Ok(Self::CodexCompact)
        } else {
            Err(report!("unsupported agg codex command"))
        }
    }
}

impl TryFrom<Arguments> for Cmd {
    type Error = Report;

    fn try_from(mut args: Arguments) -> Result<Self, Self::Error> {
        if args.contains("--help") {
            return Ok(Self::Help(Help::from_args(&args.clone().finish())));
        }

        let Some(command) = args.subcommand()? else {
            return if args.finish().is_empty() {
                Ok(Self::SessionsList)
            } else {
                Err(report!("unsupported agg command"))
            };
        };

        match command.as_str() {
            "sessions" => Self::parse_sessions(args),
            "codex" => Self::parse_codex(args),
            "sync-harness" => Ok(Self::SyncHarness(sync_harness::Opts::try_from(args.finish())?)),
            "tok" => Ok(Self::Tok(Opts::try_from(args.finish())?)),
            _ => Err(report!("unsupported agg command")),
        }
    }
}

fn strings(args: Vec<OsString>) -> Vec<String> {
    args.into_iter().map(|arg| arg.to_string_lossy().into_owned()).collect()
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;
    use ytil_agents::agent::Agent;

    use super::*;
    use crate::cmds::tok::Input;

    #[rstest::rstest]
    #[case::bare(&[], Cmd::SessionsList)]
    #[case::sessions_list(&["sessions", "list"], Cmd::SessionsList)]
    #[case::codex_compact(&["codex", "--compact"], Cmd::CodexCompact)]
    #[case::help(&["sessions", "--help"], Cmd::Help(Help::Sessions))]
    #[case::sync_harness_help(&["sync-harness", "--help"], Cmd::Help(Help::SyncHarness))]
    #[case::tok_file(&["tok", "prompt.txt"], Cmd::Tok(Opts {
        encoding: "o200k_base".to_owned(),
        input: Input::File(std::path::PathBuf::from("prompt.txt")),
    }))]
    #[case::tok_text(&["tok", "--text", "hello"], Cmd::Tok(Opts {
        encoding: "o200k_base".to_owned(),
        input: Input::Text("hello".to_owned()),
    }))]
    #[case::tok_encoding(&["tok", "--encoding", "cl100k_base", "-"], Cmd::Tok(Opts {
        encoding: "cl100k_base".to_owned(),
        input: Input::Stdin,
    }))]
    fn test_parse_known_commands(#[case] args: &[&str], #[case] expected: Cmd) {
        assert_that!(parse(args), ok(eq(expected)));
    }

    #[test]
    fn test_parse_sessions_list_json_keeps_session_args() {
        assert_that!(
            parse(&["sessions", "list", "--json", "--session", "codex:session-id"]),
            ok(eq(Cmd::SessionsListJson(vec![
                "--session".to_owned(),
                "codex:session-id".to_owned()
            ])))
        );
    }

    #[rstest::rstest]
    #[case::unknown(&["unknown"])]
    #[case::missing_sessions_subcommand(&["sessions"])]
    #[case::unexpected_sessions_list_arg(&["sessions", "list", "unexpected"])]
    #[case::missing_codex_flag(&["codex"])]
    fn test_parse_rejects_invalid_commands(#[case] args: &[&str]) {
        assert_that!(parse(args), err(anything()));
    }

    #[rstest::rstest]
    #[case::single("codex", vec![Agent::Codex])]
    #[case::all("claude,codex,cursor,gemini,opencode", vec![
        Agent::Claude,
        Agent::Codex,
        Agent::Cursor,
        Agent::Gemini,
        Agent::Opencode,
    ])]
    fn test_sync_harness_when_agents_are_supported_parses_arguments(
        #[case] agents: &str,
        #[case] expected: Vec<Agent>,
    ) {
        let args = ["sync-harness", "ai config", agents];

        let actual = parse(&args);

        assert_that!(
            actual,
            ok(eq(Cmd::SyncHarness(sync_harness::Opts {
                src_dir: std::path::PathBuf::from("ai config"),
                agents: expected,
            })))
        );
    }

    #[rstest::rstest]
    #[case::missing_all(&["sync-harness"])]
    #[case::missing_agents(&["sync-harness", "config"])]
    #[case::extra_argument(&["sync-harness", "config", "codex", "claude"])]
    #[case::empty_source(&["sync-harness", "", "codex"])]
    #[case::empty_agents(&["sync-harness", "config", ""])]
    #[case::unknown_agent(&["sync-harness", "config", "codex,unknown"])]
    #[case::empty_agent(&["sync-harness", "config", "codex,,claude"])]
    #[case::trailing_comma(&["sync-harness", "config", "codex,"])]
    fn test_sync_harness_when_arguments_are_invalid_rejects_command(#[case] args: &[&str]) {
        let actual = parse(args);

        assert_that!(actual, err(anything()));
    }

    #[rstest::rstest]
    #[case::root(&["--help"], Help::Root)]
    #[case::sessions(&["sessions", "--help"], Help::Sessions)]
    #[case::sessions_list(&["sessions", "list", "--help"], Help::SessionsList)]
    #[case::codex(&["codex", "--help"], Help::Codex)]
    #[case::sync_harness(&["sync-harness", "--help"], Help::SyncHarness)]
    #[case::tok(&["tok", "--help"], Help::Tok)]
    fn test_help_when_command_path_varies_selects_the_deepest_command(#[case] args: &[&str], #[case] expected: Help) {
        let raw = args.iter().map(OsString::from).collect::<Vec<_>>();
        assert_that!(Help::from_args(&raw), eq(expected));
    }

    fn parse(args: &[&str]) -> rootcause::Result<Cmd> {
        Cmd::try_from(Arguments::from_vec(args.iter().map(OsString::from).collect()))
    }
}
