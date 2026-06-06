use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;

use chrono::DateTime;
use chrono::Local;
use crossterm::style::Stylize;
use muxr_core::ClientRequest;
use muxr_core::ServerEvent;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use muxr_transport::ClientConnection;
use rootcause::prelude::ResultExt;

pub const SESSION_PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// Current muxr session state as observed by the local session picker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    Live,
    Stopped,
    Unknown,
}

impl fmt::Display for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Live => f.write_str("live"),
            Self::Stopped => f.write_str("stopped"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListedSession {
    created_at: Option<SystemTime>,
    name: SessionName,
    state: SessionState,
}

impl ListedSession {
    pub const fn name(&self) -> &SessionName {
        &self.name
    }

    pub const fn state(&self) -> SessionState {
        self.state
    }

    /// Render this session for ANSI-aware TUI pickers.
    pub fn display_text(&self) -> String {
        let state = match self.state {
            SessionState::Live => self.state.to_string().green().bold().to_string(),
            SessionState::Stopped => self.state.to_string().yellow().dim().to_string(),
            SessionState::Unknown => self.state.to_string().red().bold().to_string(),
        };
        format!("{} [{state}] {}", self.name, self::created_at_text(self.created_at))
    }

    pub fn search_text(&self) -> String {
        format!(
            "{} {} {}",
            self.name,
            self.state,
            self::created_at_text(self.created_at)
        )
    }
}

impl fmt::Display for ListedSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{}] {}",
            self.name,
            self.state,
            self::created_at_text(self.created_at)
        )
    }
}

/// List valid persisted muxr sessions from the local muxr state root.
///
/// # Errors
/// - `HOME` is unavailable.
/// - The session root cannot be read for reasons other than being absent.
pub fn list_sessions() -> rootcause::Result<Vec<ListedSession>> {
    self::list_sessions_from_root(&SessionPaths::sessions_root_from_home()?)
}

pub fn session_state(paths: &SessionPaths) -> rootcause::Result<SessionState> {
    if !paths.socket.exists() {
        if paths.pid.exists() {
            return Ok(SessionState::Unknown);
        }
        return Ok(SessionState::Stopped);
    }

    tokio::runtime::Runtime::new()
        .context("failed to build muxr tokio runtime")?
        .block_on(self::session_state_async(paths))
}

fn list_sessions_from_root(root: &Path) -> rootcause::Result<Vec<ListedSession>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("failed to read muxr sessions root")?,
    };

    let mut sessions = Vec::new();
    for entry in entries {
        let entry = entry.context("failed to read muxr session dir entry")?;
        if !entry
            .file_type()
            .context("failed to inspect muxr session dir entry")?
            .is_dir()
        {
            continue;
        }
        let raw_name = entry.file_name();
        let Some(raw_name) = raw_name.to_str() else {
            continue;
        };
        let Ok(name) = raw_name.parse() else {
            continue;
        };
        let paths = SessionPaths::from_sessions_root_path(root, &name)?;
        let created_at = entry
            .metadata()
            .context("failed to inspect muxr session dir metadata")?
            .created()
            .ok();
        sessions.push(ListedSession {
            created_at,
            name,
            state: self::session_state(&paths)?,
        });
    }

    sessions.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.name.as_ref().cmp(right.name.as_ref()))
    });
    Ok(sessions)
}

fn created_at_text(created_at: Option<SystemTime>) -> String {
    created_at.map_or_else(
        || "unknown".to_owned(),
        |created_at| {
            DateTime::<Local>::from(created_at)
                .format("%d-%m-%Y %H:%M:%S")
                .to_string()
        },
    )
}

/// Probe a muxr session socket without starting a nested Tokio runtime.
///
/// # Errors
/// - The probe runtime IO fails after connecting to the socket.
pub async fn session_state_async(paths: &SessionPaths) -> rootcause::Result<SessionState> {
    let Ok(Ok(mut connection)) =
        tokio::time::timeout(SESSION_PROBE_TIMEOUT, ClientConnection::connect(&paths.socket)).await
    else {
        return Ok(SessionState::Unknown);
    };

    match tokio::time::timeout(SESSION_PROBE_TIMEOUT, connection.send_request(&ClientRequest::Ping)).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) | Err(_) => return Ok(SessionState::Unknown),
    }

    if matches!(
        tokio::time::timeout(SESSION_PROBE_TIMEOUT, connection.recv_event()).await,
        Ok(Ok(Some(ServerEvent::Pong)))
    ) {
        Ok(SessionState::Live)
    } else {
        Ok(SessionState::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;

    use super::*;

    #[test]
    fn test_list_sessions_from_root_when_entries_exist_returns_valid_sessions_sorted_by_recent() -> rootcause::Result<()>
    {
        let root = tempfile::tempdir().context("failed to create muxr sessions test root")?;
        fs::create_dir(root.path().join("older")).context("failed to create older session")?;
        std::thread::sleep(Duration::from_millis(5));
        fs::create_dir(root.path().join("newer")).context("failed to create newer session")?;
        fs::create_dir(root.path().join("bad name")).context("failed to create invalid session")?;
        fs::write(root.path().join("file"), b"ignored").context("failed to create ignored file")?;

        let sessions = self::list_sessions_from_root(root.path())?;

        let rendered = sessions.iter().map(ToString::to_string).collect::<Vec<_>>();
        let expected = sessions
            .iter()
            .map(|session| {
                format!(
                    "{} [stopped] {}",
                    session.name,
                    self::created_at_text(session.created_at)
                )
            })
            .collect::<Vec<_>>();
        pretty_assertions::assert_eq!(
            sessions
                .iter()
                .map(|session| session.name.to_string())
                .collect::<Vec<_>>(),
            vec!["newer", "older"]
        );
        pretty_assertions::assert_eq!(rendered, expected);
        Ok(())
    }

    #[rstest]
    #[case::live(SessionState::Live, format!("work [{}]", "live".green().bold()))]
    #[case::stopped(SessionState::Stopped, format!("work [{}]", "stopped".yellow().dim()))]
    #[case::unknown(SessionState::Unknown, format!("work [{}]", "unknown".red().bold()))]
    fn test_listed_session_display_text_when_state_varies_colors_state(
        #[case] state: SessionState,
        #[case] expected: String,
    ) -> rootcause::Result<()> {
        let created_at = SystemTime::now();
        let session = ListedSession {
            created_at: Some(created_at),
            name: "work".parse()?,
            state,
        };

        pretty_assertions::assert_eq!(
            session.display_text(),
            format!("{expected} {}", self::created_at_text(Some(created_at)))
        );
        Ok(())
    }

    #[test]
    fn test_listed_session_display_text_when_created_at_missing_reports_unknown() -> rootcause::Result<()> {
        let session = ListedSession {
            created_at: None,
            name: "work".parse()?,
            state: SessionState::Stopped,
        };

        pretty_assertions::assert_eq!(
            session.display_text(),
            format!("work [{}] unknown", "stopped".yellow().dim())
        );
        Ok(())
    }
}
