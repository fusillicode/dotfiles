use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::PathBuf;

use rootcause::report;

use crate::EXTERNAL_LAYOUT_ARG;
use crate::SessionName;

/// Private `muxr-server` argument contract shared by the public CLI, client spawn path, and server runner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerRunnerArgs {
    /// Optional external layout file used only when creating a new session.
    pub external_layout: Option<PathBuf>,
    /// Session name owned by the server runner.
    pub session: SessionName,
}

impl ServerRunnerArgs {
    /// Parse the private server-runner argument contract used by the public `muxr` CLI.
    ///
    /// # Errors
    /// - The invocation is missing its session name, has extra args, or has an invalid session name.
    pub fn parse(args: &[OsString]) -> rootcause::Result<Self> {
        match args {
            [session] => Ok(Self {
                external_layout: None,
                session: self::parse_session_arg(session)?,
            }),
            [session, layout_flag, layout] if layout_flag == OsStr::new(EXTERNAL_LAYOUT_ARG) => Ok(Self {
                external_layout: Some(PathBuf::from(layout.clone())),
                session: self::parse_session_arg(session)?,
            }),
            [session, layout_flag] if layout_flag == OsStr::new(EXTERNAL_LAYOUT_ARG) => {
                Err(report!("missing muxr server layout").attach(format!("session={}", session.display())))
            }
            [] => Err(report!("missing muxr server session")),
            rest => Err(report!("unexpected muxr server args").attach(format!("args={rest:?}"))),
        }
    }

    /// Return argv for `muxr-server`; keep this symmetric with [`Self::parse`].
    #[must_use]
    pub fn argv(&self) -> Vec<OsString> {
        let mut args = vec![OsString::from(self.session.as_ref())];
        if let Some(external_layout) = &self.external_layout {
            args.push(OsString::from(EXTERNAL_LAYOUT_ARG));
            args.push(external_layout.as_os_str().to_owned());
        }
        args
    }
}

fn parse_session_arg(raw: &OsStr) -> rootcause::Result<SessionName> {
    let Some(raw) = raw.to_str() else {
        return Err(report!("invalid muxr server session").attach("reason=session must be valid UTF-8"));
    };
    raw.parse()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use rstest::rstest;

    use super::*;

    #[test]
    fn test_parse_when_session_is_supplied_returns_session() -> rootcause::Result<()> {
        let args = ServerRunnerArgs::parse(&args(&["work"]))?;

        pretty_assertions::assert_eq!(args.session.as_ref(), "work");
        pretty_assertions::assert_eq!(args.external_layout, None);
        Ok(())
    }

    #[test]
    fn test_parse_when_layout_is_supplied_returns_layout() -> rootcause::Result<()> {
        let args = ServerRunnerArgs::parse(&args(&["work", "--layout", ".config/muxr/layouts/work.json"]))?;

        pretty_assertions::assert_eq!(args.session.as_ref(), "work");
        pretty_assertions::assert_eq!(
            args.external_layout.as_deref().and_then(Path::to_str),
            Some(".config/muxr/layouts/work.json")
        );
        Ok(())
    }

    #[test]
    fn test_argv_when_layout_is_supplied_returns_runner_args() -> rootcause::Result<()> {
        let args = ServerRunnerArgs {
            external_layout: Some(PathBuf::from(".config/muxr/layouts/work.json")),
            session: "work".parse()?,
        };

        pretty_assertions::assert_eq!(
            args.argv(),
            vec![
                OsString::from("work"),
                OsString::from(EXTERNAL_LAYOUT_ARG),
                OsStr::new(".config/muxr/layouts/work.json").to_owned()
            ]
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_parse_when_layout_is_not_utf8_preserves_path_bytes() -> rootcause::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::ffi::OsStringExt;

        let raw_layout = OsString::from_vec(b"layout-\xFF.json".to_vec());
        let parsed =
            ServerRunnerArgs::parse(&[OsString::from("work"), OsString::from(EXTERNAL_LAYOUT_ARG), raw_layout])?;

        pretty_assertions::assert_eq!(parsed.session.as_ref(), "work");
        pretty_assertions::assert_eq!(
            parsed
                .external_layout
                .as_deref()
                .map(Path::as_os_str)
                .map(OsStr::as_bytes),
            Some(b"layout-\xFF.json".as_slice())
        );
        Ok(())
    }

    #[rstest]
    #[case::missing_session(&[])]
    #[case::extra_args(&["work", "extra"])]
    #[case::missing_layout(&["work", "--layout"])]
    fn test_parse_when_args_are_invalid_returns_error(#[case] raw: &[&str]) {
        assert2::assert!(ServerRunnerArgs::parse(&args(raw)).is_err());
    }

    fn args(raw: &[&str]) -> Vec<OsString> {
        raw.iter().map(OsString::from).collect()
    }
}
