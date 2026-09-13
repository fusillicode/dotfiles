use std::path::PathBuf;

use muxr_core::EXTERNAL_LAYOUT_ARG;
use muxr_core::SessionName;
use rootcause::report;

pub mod sessions;
pub mod start;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Cmd {
    Help,
    Sessions,
    Start {
        session: SessionName,
        external_layout: Option<PathBuf>,
    },
}

impl Cmd {
    pub fn from_env() -> rootcause::Result<Self> {
        Self::parse(&ytil_sys::cli::get())
    }

    fn parse(args: &[String]) -> rootcause::Result<Self> {
        if args.iter().any(|arg| arg == "--help") {
            return Ok(Self::Help);
        }

        match args {
            [] => Ok(Self::Sessions),
            [command, rest @ ..] if command == "start" => Self::parse_start(rest),
            [command, ..] => Err(report!("unknown muxr cmd {command:?}")),
        }
    }

    fn parse_start(args: &[String]) -> rootcause::Result<Self> {
        match args {
            [] => Ok(Self::Start {
                session: SessionName::default(),
                external_layout: None,
            }),
            [layout_flag, layout] if layout_flag == EXTERNAL_LAYOUT_ARG => Ok(Self::Start {
                session: SessionName::default(),
                external_layout: Some(PathBuf::from(layout)),
            }),
            [layout_flag] if layout_flag == EXTERNAL_LAYOUT_ARG => {
                Err(report!("missing muxr start layout").attach(format!("flag={EXTERNAL_LAYOUT_ARG}")))
            }
            [session] => Ok(Self::Start {
                session: session.parse()?,
                external_layout: None,
            }),
            [session, layout_flag, layout] if layout_flag == EXTERNAL_LAYOUT_ARG => Ok(Self::Start {
                session: session.parse()?,
                external_layout: Some(PathBuf::from(layout)),
            }),
            [session, layout_flag] if layout_flag == EXTERNAL_LAYOUT_ARG => {
                Err(report!("missing muxr start layout").attach(format!("session={session:?}")))
            }
            _ => Err(report!("unexpected muxr start args").attach(format!("args={args:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rstest::rstest;
    use test_that::prelude::*;

    use super::*;

    #[rstest]
    #[case::start_without_session(&["start"], "default", None)]
    #[case::start_with_session(&["start", "work"], "work", None)]
    #[case::start_default_with_layout(
        &["start", "--layout", "../.config/muxr/layouts/work.json"],
        "default",
        Some("../.config/muxr/layouts/work.json")
    )]
    #[case::start_session_with_layout(
        &["start", "work", "--layout", ".config/muxr/layouts/work.json"],
        "work",
        Some(".config/muxr/layouts/work.json")
    )]
    fn test_parse_when_start_args_vary_returns_start_cmd(
        #[case] raw: &[&str],
        #[case] expected_session: &str,
        #[case] expected_layout: Option<&str>,
    ) -> rootcause::Result<()> {
        let parsed = Cmd::parse(&args(raw))?;
        let Cmd::Start {
            session,
            external_layout,
        } = parsed
        else {
            assert_that!(
                parsed,
                matches_pattern!(Cmd::Start {
                    session: anything(),
                    external_layout: anything()
                })
            );
            return Ok(());
        };
        assert_that!(session.as_ref(), eq(expected_session));
        assert_that!(external_layout.as_deref().and_then(Path::to_str), eq(expected_layout));
        Ok(())
    }

    #[rstest]
    #[case::help_arg(&["--help"])]
    #[case::help_among_args(&["start", "--help"])]
    fn test_parse_when_help_requested_returns_help(#[case] raw: &[&str]) -> rootcause::Result<()> {
        assert_that!(Cmd::parse(&args(raw))?, eq(Cmd::Help));
        Ok(())
    }

    #[test]
    fn test_parse_when_no_args_returns_session_picker() -> rootcause::Result<()> {
        assert_that!(Cmd::parse(&args(&[]))?, eq(Cmd::Sessions));
        Ok(())
    }

    #[rstest]
    #[case::start_extra_args(&["start", "work", "extra"])]
    #[case::start_missing_layout(&["start", "--layout"])]
    #[case::start_session_missing_layout(&["start", "work", "--layout"])]
    #[case::start_layout_extra_args(&["start", "work", "--layout", "work", "extra"])]
    #[case::old_memory_cmd(&["memory"])]
    #[case::unknown_start_flag(&["start", "--bogus"])]
    #[case::old_attach_cmd(&["attach"])]
    #[case::old_detach_cmd(&["detach"])]
    #[case::old_server_cmd(&["server", "work"])]
    #[case::unknown_cmd(&["bogus"])]
    fn test_parse_when_args_are_invalid_returns_error(#[case] raw: &[&str]) {
        assert_that!(Cmd::parse(&args(raw)), err(anything()));
    }

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(ToString::to_string).collect()
    }
}
