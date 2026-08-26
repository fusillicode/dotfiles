use std::env;
use std::fmt;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use rootcause::report;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

pub const DEFAULT_SESSION_NAME: &str = "default";
pub const EXTERNAL_LAYOUT_ARG: &str = "--layout";
/// Timestamp format used in muxr server log filenames.
///
/// The server owns timestamp generation; clients should not pass this through the private runner argv.
pub const SERVER_LOG_TIMESTAMP_FORMAT: &str = "%Y%m%d%H%M%S";
const STATE_HOME_PARTS: &[&str] = &[".local", "state", "muxr"];

const LOGS_DIR_NAME: &str = "logs";
const SOCKET_HOME_PARTS: &[&str] = &["s"];
const SOCKET_HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const SOCKET_HASH_PRIME: u64 = 0x0000_0100_0000_01b3;
const SOCKET_PATH_MAX_BYTES: usize = 103;
const SERVER_LOG_TIMESTAMP_LEN: usize = 14;

/// Validated timestamp component for a muxr server log filename.
///
/// This is intentionally a filename-only type, not a protocol/versioning field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerLogTimestamp(String);

impl AsRef<str> for ServerLogTimestamp {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServerLogTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for ServerLogTimestamp {
    type Err = rootcause::Report;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw.len() != SERVER_LOG_TIMESTAMP_LEN {
            return Err(report!("invalid muxr server log timestamp {raw:?}").attach("reason=expected YYYYMMDDHHMMSS"));
        }
        let bytes = raw.as_bytes();
        if !bytes.iter().all(u8::is_ascii_digit) {
            return Err(report!("invalid muxr server log timestamp {raw:?}").attach("reason=expected YYYYMMDDHHMMSS"));
        }
        Ok(Self(raw.to_owned()))
    }
}

#[derive(rkyv::Archive, Clone, Debug, Eq, Hash, PartialEq, Serialize, rkyv::Serialize)]
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
        self::validate_muxr_name(raw, "session")?;
        Ok(Self(raw.to_owned()))
    }
}

impl<D> rkyv::Deserialize<SessionName, D> for ArchivedSessionName
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<SessionName, D::Error> {
        let raw = rkyv::Deserialize::<String, D>::deserialize(&self.0, deserializer)?;
        raw.parse().map_err(|error: rootcause::Report| {
            <D::Error as rkyv::rancor::Source>::new(io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
        })
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
    /// Return the root directory containing all muxr session directories under `$HOME`.
    ///
    /// # Errors
    /// - `HOME` is unavailable in the current environment.
    pub fn sessions_root_from_home() -> rootcause::Result<PathBuf> {
        let home = env::var_os("HOME").ok_or_else(|| report!("missing HOME env for muxr state root"))?;
        Ok(Self::sessions_root_from_home_path(PathBuf::from(home)))
    }

    /// Return the root directory containing all muxr session directories under an explicit home path.
    #[must_use]
    pub fn sessions_root_from_home_path(home: PathBuf) -> PathBuf {
        Self::state_root_from_home_path(home).join("sessions")
    }

    /// Build muxr session paths under `$HOME/.local/state/muxr`.
    ///
    /// # Errors
    /// - `HOME` is unavailable in the current environment.
    /// - The derived Unix socket path is too long for the platform socket address.
    pub fn from_home(session: &SessionName) -> rootcause::Result<Self> {
        let home = env::var_os("HOME").ok_or_else(|| report!("missing HOME env for muxr state root"))?;
        Self::from_home_path(PathBuf::from(home), session)
    }

    /// Build muxr session paths from an explicit sessions root.
    ///
    /// # Errors
    /// - The sessions root has no parent state directory.
    /// - The derived Unix socket path is too long for the platform socket address.
    pub fn from_sessions_root_path(sessions_root: &Path, session: &SessionName) -> rootcause::Result<Self> {
        let state_root = sessions_root
            .parent()
            .ok_or_else(|| report!("muxr sessions root has no parent"))?;
        Self::from_state_root_path(state_root, session)
    }

    /// Build the centralized directory containing muxr server logs.
    ///
    /// # Errors
    /// - The session root path has no parent state root.
    pub fn logs_root(&self) -> rootcause::Result<PathBuf> {
        Ok(self.state_root()?.join(LOGS_DIR_NAME))
    }

    /// Build the centralized server log path for one muxr server process start.
    ///
    /// # Errors
    /// - The session root path has no parent state root.
    pub fn server_log_path(
        &self,
        session: &SessionName,
        timestamp: &ServerLogTimestamp,
        pid: u32,
    ) -> rootcause::Result<PathBuf> {
        Ok(self
            .logs_root()?
            .join(self::server_log_file_name(session, timestamp, pid)))
    }

    /// Return the pid-scoped server log filename pattern used in client startup failure hints.
    ///
    /// The timestamp is chosen inside the server, so the client can only know the session name and spawned pid.
    #[must_use]
    pub fn server_log_file_pattern(session: &SessionName, pid: u32) -> String {
        format!("{session}-*-{pid}.log")
    }

    fn from_home_path(home: PathBuf, session: &SessionName) -> rootcause::Result<Self> {
        Self::from_state_root_path(&Self::state_root_from_home_path(home), session)
    }

    fn from_state_root_path(state_root: &Path, session: &SessionName) -> rootcause::Result<Self> {
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

    fn state_root_from_home_path(home: PathBuf) -> PathBuf {
        STATE_HOME_PARTS.iter().fold(home, |path, part| path.join(part))
    }

    fn state_root(&self) -> rootcause::Result<&Path> {
        self.root
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| report!("muxr session root has no state parent"))
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

fn server_log_file_name(session: &SessionName, timestamp: &ServerLogTimestamp, pid: u32) -> String {
    format!("{session}-{timestamp}-{pid}.log")
}

fn validate_muxr_name(raw: &str, kind: &str) -> rootcause::Result<()> {
    if raw.is_empty() {
        return Err(report!("invalid muxr {kind} name {raw:?}").attach("reason=empty names are not allowed"));
    }

    if matches!(raw, "." | "..") {
        return Err(report!("invalid muxr {kind} name {raw:?}").attach("reason=reserved names are not allowed"));
    }

    if raw.starts_with('-') {
        // Names are CLI operands too; leading '-' is reserved for flags before the value reaches filesystem paths.
        return Err(report!("invalid muxr {kind} name {raw:?}").attach("reason=names must not start with -"));
    }

    if raw.len() > 64 {
        return Err(
            report!("invalid muxr {kind} name {raw:?}").attach("reason=names longer than 64 bytes are not allowed")
        );
    }

    if !raw
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(report!("invalid muxr {kind} name {raw:?}")
            .attach("reason=only ASCII alphanumeric, _, -, and . are allowed"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rstest::rstest;
    use test_that::prelude::*;

    use super::*;

    #[rstest]
    #[case::default("default")]
    #[case::work("work")]
    #[case::hyphen("a-b")]
    #[case::underscore("a_b")]
    #[case::dot("a.b")]
    #[case::alphanumeric("abc123")]
    fn test_session_name_from_str_when_name_is_valid_returns_session_name(#[case] raw: &str) -> rootcause::Result<()> {
        assert_that!(raw.parse::<SessionName>()?.as_ref(), eq(raw));
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
    #[case::leading_dash("-work")]
    #[case::flag_like("--work")]
    #[case::shell_metacharacters("$(x)")]
    #[case::punctuation("name!")]
    fn test_session_name_from_str_when_name_is_invalid_returns_error(#[case] raw: &str) {
        assert_that!(
            raw.parse::<SessionName>(),
            err(displays_as(contains_substring(format!("{raw:?}"))))
        );
    }

    #[test]
    fn test_session_name_rkyv_deserialize_when_name_is_invalid_returns_error() -> rootcause::Result<()> {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&SessionName("../x".to_owned()))?;
        let archived = rkyv::access::<rkyv::Archived<SessionName>, rkyv::rancor::Error>(&bytes)?;

        assert_that!(
            rkyv::deserialize::<SessionName, rkyv::rancor::Error>(archived),
            err(anything())
        );
        Ok(())
    }

    #[test]
    fn test_session_name_default_returns_default_session() {
        assert_that!(SessionName::default().as_ref(), eq(DEFAULT_SESSION_NAME));
    }

    #[rstest]
    #[case::midnight("20260611000000")]
    #[case::with_time("20260611143012")]
    fn test_server_log_timestamp_from_str_when_timestamp_is_valid_returns_timestamp(
        #[case] raw: &str,
    ) -> rootcause::Result<()> {
        assert_that!(raw.parse::<ServerLogTimestamp>()?.as_ref(), eq(raw));
        Ok(())
    }

    #[rstest]
    #[case::empty("")]
    #[case::epoch_millis("1781181012000")]
    #[case::old_separator("20260611-143012")]
    #[case::slash("20260611/143012")]
    #[case::letters("20260611abcdef")]
    fn test_server_log_timestamp_from_str_when_timestamp_is_invalid_returns_error(#[case] raw: &str) {
        assert_that!(raw.parse::<ServerLogTimestamp>(), err(anything()));
    }

    #[test]
    fn test_session_paths_from_home_builds_expected_paths() -> rootcause::Result<()> {
        let home = Path::new("/foo/bar");
        let session = "work".parse()?;
        let state_root = home.join(".local").join("state").join("muxr");
        let root = state_root.join("sessions").join("work");

        let paths = SessionPaths::from_home_path(home.to_path_buf(), &session)?;

        assert_that!(
            paths,
            eq(SessionPaths {
                socket: state_root.join("s").join(self::socket_file_name(&session)),
                pid: root.join("server.pid"),
                layout: root.join("layout.json"),
                panes: root.join("panes"),
                root,
            })
        );
        assert_that!(
            paths.socket.as_os_str().as_encoded_bytes().len(),
            le(SOCKET_PATH_MAX_BYTES)
        );
        Ok(())
    }

    #[test]
    fn test_session_paths_logs_root_returns_state_logs_path() -> rootcause::Result<()> {
        let session = "work".parse()?;
        let paths = SessionPaths::from_home_path(Path::new("/foo/bar").to_path_buf(), &session)?;

        assert_that!(
            paths.logs_root()?,
            eq(Path::new("/foo/bar")
                .join(".local")
                .join("state")
                .join("muxr")
                .join("logs"))
        );
        Ok(())
    }

    #[test]
    fn test_session_paths_server_log_path_returns_centralized_flat_log_path() -> rootcause::Result<()> {
        let session = "work.review-1".parse()?;
        let timestamp = "20260611143012".parse()?;
        let paths = SessionPaths::from_home_path(Path::new("/foo/bar").to_path_buf(), &session)?;

        assert_that!(
            paths.server_log_path(&session, &timestamp, 12345)?,
            eq(Path::new("/foo/bar")
                .join(".local")
                .join("state")
                .join("muxr")
                .join("logs")
                .join("work.review-1-20260611143012-12345.log"))
        );
        Ok(())
    }

    #[test]
    fn test_session_paths_server_log_file_pattern_returns_pid_scoped_pattern() -> rootcause::Result<()> {
        let session = "work.review-1".parse()?;

        assert_that!(
            SessionPaths::server_log_file_pattern(&session, 12345),
            eq("work.review-1-*-12345.log")
        );
        Ok(())
    }

    #[test]
    fn test_session_paths_server_log_path_when_pid_differs_returns_distinct_path() -> rootcause::Result<()> {
        let session = "work".parse()?;
        let timestamp = "20260611143012".parse()?;
        let paths = SessionPaths::from_home_path(Path::new("/foo/bar").to_path_buf(), &session)?;

        assert_that!(
            paths.server_log_path(&session, &timestamp, 12345)?,
            not(eq(paths.server_log_path(&session, &timestamp, 12346)?))
        );
        Ok(())
    }

    #[test]
    fn test_session_paths_from_home_path_when_session_name_is_max_length_keeps_socket_short() -> rootcause::Result<()> {
        let session = "a".repeat(64).parse()?;
        let paths = SessionPaths::from_home_path(Path::new("/foo/bar").to_path_buf(), &session)?;

        assert_that!(
            paths.root,
            eq(Path::new("/foo/bar")
                .join(".local")
                .join("state")
                .join("muxr")
                .join("sessions")
                .join(session.as_ref()))
        );
        assert_that!(
            paths.socket.as_os_str().as_encoded_bytes().len(),
            le(SOCKET_PATH_MAX_BYTES)
        );
        Ok(())
    }

    #[test]
    fn test_session_paths_from_home_path_when_socket_path_is_too_long_returns_error() -> rootcause::Result<()> {
        let home = Path::new("/").join("x".repeat(SOCKET_PATH_MAX_BYTES.saturating_add(1)));
        let session = "work".parse()?;

        let error = SessionPaths::from_home_path(home, &session).expect_err("expected socket path length error");

        assert_that!(error.to_string(), contains_substring("muxr socket path is too long"));
        Ok(())
    }

    #[test]
    fn test_validate_socket_path_when_path_is_too_long_returns_error() {
        let path = Path::new("/").join("x".repeat(SOCKET_PATH_MAX_BYTES.saturating_add(1)));

        assert_that!(self::validate_socket_path(&path), err(anything()));
    }
}
