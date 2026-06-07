use std::path::PathBuf;

use muxr_core::EXTERNAL_LAYOUT_ARG;
use muxr_core::INTERNAL_SERVER_ARG;
use muxr_core::SessionName;
use rootcause::report;

#[derive(Clone, Debug, Eq, PartialEq)]
struct InternalServerArgs {
    session: SessionName,
    external_layout: Option<PathBuf>,
}

impl InternalServerArgs {
    /// Parse hidden internal server arguments.
    ///
    /// # Errors
    /// - The internal server invocation is missing its session name, has extra args, or has an invalid session name.
    fn parse(args: &[String]) -> rootcause::Result<Option<Self>> {
        match args {
            [flag, session] if flag == INTERNAL_SERVER_ARG => Ok(Some(Self {
                session: session.parse()?,
                external_layout: None,
            })),
            [flag, session, layout_flag, layout]
                if flag == INTERNAL_SERVER_ARG && layout_flag == EXTERNAL_LAYOUT_ARG =>
            {
                Ok(Some(Self {
                    session: session.parse()?,
                    external_layout: Some(PathBuf::from(layout)),
                }))
            }
            [flag, rest @ ..] if flag == INTERNAL_SERVER_ARG => {
                Err(report!("unexpected muxr internal server args").attach(format!("args={rest:?}")))
            }
            _ => Ok(None),
        }
    }
}

pub fn serve_if_requested(args: &[String]) -> rootcause::Result<bool> {
    let Some(internal) = InternalServerArgs::parse(args)? else {
        return Ok(false);
    };

    muxr_server::serve_session(&internal.session, internal.external_layout)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rstest::rstest;

    use super::*;

    #[test]
    fn test_parse_when_server_arg_has_session_returns_session() -> rootcause::Result<()> {
        let Some(internal) = InternalServerArgs::parse(&args(&["--server", "work"]))? else {
            return Err(report!("expected internal server session"));
        };

        pretty_assertions::assert_eq!(internal.session.as_ref(), "work");
        pretty_assertions::assert_eq!(internal.external_layout, None);
        Ok(())
    }

    #[test]
    fn test_parse_when_layout_is_supplied_returns_layout() -> rootcause::Result<()> {
        let Some(internal) = InternalServerArgs::parse(&args(&[
            "--server",
            "work",
            "--layout",
            ".config/muxr/layouts/work.json",
        ]))?
        else {
            return Err(report!("expected internal server session"));
        };

        pretty_assertions::assert_eq!(internal.session.as_ref(), "work");
        pretty_assertions::assert_eq!(
            internal.external_layout.as_deref().and_then(Path::to_str),
            Some(".config/muxr/layouts/work.json")
        );
        Ok(())
    }

    #[rstest]
    #[case::missing_session(&["--server"])]
    #[case::extra_args(&["--server", "work", "extra"])]
    #[case::missing_layout(&["--server", "work", "--layout"])]
    fn test_parse_when_args_are_invalid_returns_error(#[case] raw: &[&str]) {
        assert2::assert!(InternalServerArgs::parse(&args(raw)).is_err());
    }

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(ToString::to_string).collect()
    }
}
