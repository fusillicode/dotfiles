use std::env;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use rootcause::report;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

pub const DEFAULT_SESSION_NAME: &str = "default";
pub const INTERNAL_SERVER_ARG: &str = "--server";
pub const STATE_HOME_PARTS: &[&str] = &[".local", "state", "muxr"];

const SOCKET_HOME_PARTS: &[&str] = &["s"];
const SOCKET_HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const SOCKET_HASH_PRIME: u64 = 0x0000_0100_0000_01b3;
const SOCKET_PATH_MAX_BYTES: usize = 103;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SessionName(String);

impl<'de> Deserialize<'de> for SessionName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl AsRef<str> for SessionName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Default for SessionName {
    fn default() -> Self {
        Self(DEFAULT_SESSION_NAME.to_owned())
    }
}

impl fmt::Display for SessionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for SessionName {
    type Err = rootcause::Report;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw.is_empty() {
            return Err(report!("invalid muxr session name {raw:?}").attach("reason=empty names are not allowed"));
        }

        if matches!(raw, "." | "..") {
            return Err(report!("invalid muxr session name {raw:?}").attach("reason=reserved names are not allowed"));
        }

        if raw.len() > 64 {
            return Err(report!("invalid muxr session name {raw:?}")
                .attach("reason=names longer than 64 bytes are not allowed"));
        }

        if !raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        {
            return Err(report!("invalid muxr session name {raw:?}")
                .attach("reason=only ASCII alphanumeric, _, -, and . are allowed"));
        }

        Ok(Self(raw.to_owned()))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionPaths {
    pub root: PathBuf,
    pub socket: PathBuf,
    pub pid: PathBuf,
    pub layout: PathBuf,
    pub panes: PathBuf,
}

impl SessionPaths {
    /// Build muxr session paths under `$HOME/.local/state/muxr`.
    ///
    /// # Errors
    /// - `HOME` is unavailable in the current environment.
    /// - The derived Unix socket path is too long for the platform socket address.
    pub fn from_home(session: &SessionName) -> rootcause::Result<Self> {
        let home = env::var_os("HOME").ok_or_else(|| report!("missing HOME env for muxr state root"))?;
        Self::from_home_path(PathBuf::from(home), session)
    }

    fn from_home_path(home: PathBuf, session: &SessionName) -> rootcause::Result<Self> {
        let state_root = STATE_HOME_PARTS.iter().fold(home, |path, part| path.join(part));
        let root = state_root.join("sessions").join(session.as_ref());
        let socket = state_root
            .join(
                SOCKET_HOME_PARTS
                    .iter()
                    .fold(PathBuf::new(), |path, part| path.join(part)),
            )
            .join(self::socket_file_name(session));

        self::validate_socket_path(&socket)?;
        Ok(Self {
            socket,
            pid: root.join("server.pid"),
            layout: root.join("layout.json"),
            panes: root.join("panes"),
            root,
        })
    }
}

/// Validate that a muxr Unix socket path fits the portable filesystem-socket limit.
///
/// # Errors
/// - The path is longer than the conservative macOS `sockaddr_un.sun_path` capacity.
pub fn validate_socket_path(path: &Path) -> rootcause::Result<()> {
    // Filesystem Unix socket paths include a trailing NUL in sockaddr_un; 103 bytes is the safe macOS payload.
    let bytes = path.as_os_str().as_encoded_bytes().len();
    if bytes > SOCKET_PATH_MAX_BYTES {
        return Err(report!("muxr socket path is too long")
            .attach(format!("limit={SOCKET_PATH_MAX_BYTES}"))
            .attach(format!("actual={bytes}"))
            .attach(format!("path={}", path.display())));
    }

    Ok(())
}

fn socket_file_name(session: &SessionName) -> String {
    format!("{:016x}.sock", self::socket_hash(session))
}

fn socket_hash(session: &SessionName) -> u64 {
    let mut hash = SOCKET_HASH_OFFSET;
    for byte in session.as_ref().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(SOCKET_HASH_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::default("default")]
    #[case::work("work")]
    #[case::hyphen("a-b")]
    #[case::underscore("a_b")]
    #[case::dot("a.b")]
    #[case::alphanumeric("abc123")]
    fn test_session_name_from_str_when_name_is_valid_returns_session_name(#[case] raw: &str) -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(raw.parse::<SessionName>()?.as_ref(), raw);
        Ok(())
    }

    #[rstest]
    #[case::empty("")]
    #[case::dot(".")]
    #[case::dot_dot("..")]
    #[case::forward_slash("a/b")]
    #[case::backslash("a\\b")]
    #[case::space("a b")]
    #[case::tab("a\tb")]
    #[case::shell_metacharacters("$(x)")]
    #[case::punctuation("name!")]
    fn test_session_name_from_str_when_name_is_invalid_returns_error(#[case] raw: &str) {
        assert2::assert!(matches!(
            raw.parse::<SessionName>(),
            Err(ref err) if err.to_string().contains(&format!("{raw:?}"))
        ));
    }

    #[test]
    fn test_session_name_deserialize_when_name_is_invalid_returns_error() {
        assert2::assert!(serde_json::from_str::<SessionName>("\"../x\"").is_err());
    }

    #[test]
    fn test_session_name_default_returns_default_session() {
        pretty_assertions::assert_eq!(SessionName::default().as_ref(), DEFAULT_SESSION_NAME);
    }

    #[test]
    fn test_session_paths_from_home_builds_expected_paths() -> rootcause::Result<()> {
        let home = Path::new("/Users/gianlu");
        let session = "work".parse()?;
        let state_root = home.join(".local").join("state").join("muxr");
        let root = state_root.join("sessions").join("work");

        let paths = SessionPaths::from_home_path(home.to_path_buf(), &session)?;

        pretty_assertions::assert_eq!(
            paths,
            SessionPaths {
                socket: state_root.join("s").join(self::socket_file_name(&session)),
                pid: root.join("server.pid"),
                layout: root.join("layout.json"),
                panes: root.join("panes"),
                root,
            },
        );
        assert2::assert!(paths.socket.as_os_str().as_encoded_bytes().len() <= SOCKET_PATH_MAX_BYTES);
        Ok(())
    }

    #[test]
    fn test_session_paths_from_home_path_when_session_name_is_max_length_keeps_socket_short() -> rootcause::Result<()> {
        let session = "a".repeat(64).parse()?;
        let paths = SessionPaths::from_home_path(Path::new("/Users/gianlu").to_path_buf(), &session)?;

        pretty_assertions::assert_eq!(
            paths.root,
            Path::new("/Users/gianlu")
                .join(".local")
                .join("state")
                .join("muxr")
                .join("sessions")
                .join(session.as_ref()),
        );
        assert2::assert!(paths.socket.as_os_str().as_encoded_bytes().len() <= SOCKET_PATH_MAX_BYTES);
        Ok(())
    }

    #[test]
    fn test_session_paths_from_home_path_when_socket_path_is_too_long_returns_error() -> rootcause::Result<()> {
        let home = Path::new("/").join("x".repeat(SOCKET_PATH_MAX_BYTES.saturating_add(1)));
        let session = "work".parse()?;

        let error = SessionPaths::from_home_path(home, &session).expect_err("expected socket path length error");

        assert2::assert!(error.to_string().contains("muxr socket path is too long"));
        Ok(())
    }

    #[test]
    fn test_validate_socket_path_when_path_is_too_long_returns_error() {
        let path = Path::new("/").join("x".repeat(SOCKET_PATH_MAX_BYTES.saturating_add(1)));

        assert2::assert!(self::validate_socket_path(&path).is_err());
    }
}
