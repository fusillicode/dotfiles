use std::ffi::OsString;

use rootcause::report;
use ytil_sys::pico_args::Arguments;

use crate::cmds::tok::Opts;

pub mod codex;
pub mod sessions;
pub mod tok;

#[derive(Debug, Eq, PartialEq)]
pub enum Cmd {
    Help,
    SessionsList,
    SessionsListJson(Vec<String>),
    CodexCompact,
    Tok(Opts),
}

impl Cmd {
    pub fn from_env() -> rootcause::Result<Self> {
        Self::try_from(Arguments::from_env())
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
    type Error = rootcause::Report;

    fn try_from(mut args: Arguments) -> Result<Self, Self::Error> {
        if args.contains("--help") {
            return Ok(Self::Help);
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

    use super::*;
    use crate::cmds::tok::Input;
    use crate::cmds::tok::OutputFmt;

    #[rstest::rstest]
    #[case::bare(&[], Cmd::SessionsList)]
    #[case::sessions_list(&["sessions", "list"], Cmd::SessionsList)]
    #[case::codex_compact(&["codex", "--compact"], Cmd::CodexCompact)]
    #[case::help(&["sessions", "--help"], Cmd::Help)]
    #[case::tok_file(&["tok", "prompt.txt"], Cmd::Tok(Opts {
        encoding: "o200k_base".to_owned(),
        input: Input::File(std::path::PathBuf::from("prompt.txt")),
        output_fmt: OutputFmt::Json,
    }))]
    #[case::tok_text(&["tok", "--text", "hello"], Cmd::Tok(Opts {
        encoding: "o200k_base".to_owned(),
        input: Input::Text("hello".to_owned()),
        output_fmt: OutputFmt::Json,
    }))]
    #[case::tok_encoding(&["tok", "--encoding", "cl100k_base", "-"], Cmd::Tok(Opts {
        encoding: "cl100k_base".to_owned(),
        input: Input::Stdin,
        output_fmt: OutputFmt::Json,
    }))]
    #[case::tok_plain(&["tok", "--plain", "prompt.txt"], Cmd::Tok(Opts {
        encoding: "o200k_base".to_owned(),
        input: Input::File(std::path::PathBuf::from("prompt.txt")),
        output_fmt: OutputFmt::Plain,
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

    fn parse(args: &[&str]) -> rootcause::Result<Cmd> {
        Cmd::try_from(Arguments::from_vec(args.iter().map(OsString::from).collect()))
    }
}
