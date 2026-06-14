use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use muxr_core::ClientRequest;
use muxr_core::ServerEvent;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use muxr_transport::ClientConnection;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::session::list::SESSION_PROBE_TIMEOUT;
use crate::session::list::SessionState;
use crate::session::list::session_state;

// Live delete can race with one bounded server write to an attached client; keep the cleanup wait above that path.
const SESSION_DELETE_TIMEOUT: Duration = Duration::from_secs(5);
const SESSION_DELETE_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Result of deleting one selected muxr session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionDeleteOutcome {
    LiveDeleted,
    LiveVanishedForced,
    StoppedRemoved,
    UnknownForced,
}

/// Delete a muxr session selected from the local picker.
///
/// # Errors
/// - The session paths cannot be resolved.
/// - A live session accepts delete but does not stop and remove its files in time.
/// - The session files cannot be removed.
pub fn delete_session(session: &SessionName) -> rootcause::Result<SessionDeleteOutcome> {
    let paths = SessionPaths::from_home(session)?;
    self::delete_session_paths(&paths)
}

fn delete_session_paths(paths: &SessionPaths) -> rootcause::Result<SessionDeleteOutcome> {
    // A reachable server gets a protocol delete so pane processes close before state files disappear.
    match session_state(paths)? {
        SessionState::Live => match self::delete_live_session(paths) {
            Ok(()) => Ok(SessionDeleteOutcome::LiveDeleted),
            Err(error) => {
                // The live probe and delete request are separate socket operations; a server can exit between them.
                // Once the selected server is no longer reachable, deletion becomes local stopped-session cleanup.
                if session_state(paths)? == SessionState::Live {
                    return Err(error);
                }
                self::remove_session_files(paths)?;
                Ok(SessionDeleteOutcome::LiveVanishedForced)
            }
        },
        SessionState::Stopped => {
            self::remove_session_files(paths)?;
            Ok(SessionDeleteOutcome::StoppedRemoved)
        }
        SessionState::Unknown => {
            self::remove_session_files(paths)?;
            Ok(SessionDeleteOutcome::UnknownForced)
        }
    }
}

fn delete_live_session(paths: &SessionPaths) -> rootcause::Result<()> {
    tokio::runtime::Runtime::new()
        .context("failed to build muxr tokio runtime")?
        .block_on(self::delete_live_session_async(paths))?;
    self::wait_for_session_files_removed(paths)
}

async fn delete_live_session_async(paths: &SessionPaths) -> rootcause::Result<()> {
    let mut connection = tokio::time::timeout(SESSION_PROBE_TIMEOUT, ClientConnection::connect(&paths.socket))
        .await
        .map_err(|_| report!("timed out connecting live muxr session"))?
        .context("failed to connect live muxr session")?;

    tokio::time::timeout(
        SESSION_PROBE_TIMEOUT,
        connection.send_request(&ClientRequest::DeleteSession),
    )
    .await
    .map_err(|_| report!("timed out sending muxr delete request"))?
    .context("failed to send muxr delete request")?;

    match tokio::time::timeout(SESSION_DELETE_TIMEOUT, connection.recv_event())
        .await
        .map_err(|_| report!("timed out waiting for muxr delete response"))?
        .context("failed to read muxr delete response")?
    {
        Some(ServerEvent::Deleted) => Ok(()),
        Some(ServerEvent::Error(error)) => Err(report!("muxr server rejected delete")
            .attach(format!("code={}", error.code()))
            .attach(format!("msg={}", error.msg()))),
        Some(event) => Err(report!("unexpected muxr delete response").attach(format!("{event:?}"))),
        None => Err(report!("muxr server closed before delete response")),
    }
}

fn wait_for_session_files_removed(paths: &SessionPaths) -> rootcause::Result<()> {
    let started_at = Instant::now();
    loop {
        if !paths.root.exists() && !paths.socket.exists() {
            return Ok(());
        }

        if started_at.elapsed() > SESSION_DELETE_TIMEOUT {
            return Err(report!("timed out waiting for muxr session deletion")
                .attach(format!("root={}", paths.root.display()))
                .attach(format!("socket={}", paths.socket.display())));
        }

        std::thread::sleep(SESSION_DELETE_POLL_INTERVAL);
    }
}

fn remove_session_files(paths: &SessionPaths) -> rootcause::Result<()> {
    self::remove_dir_if_exists(&paths.root)?;
    self::remove_file_if_exists(&paths.socket)
}

fn remove_dir_if_exists(path: &Path) -> rootcause::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("failed to remove muxr session dir")?,
    }
}

fn remove_file_if_exists(path: &Path) -> rootcause::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("failed to remove muxr session file")?,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn test_delete_session_paths_when_session_is_stopped_removes_root() -> rootcause::Result<()> {
        let root = tempfile::tempdir().context("failed to create muxr delete test root")?;
        let session: SessionName = "dead".parse()?;
        let paths = self::test_paths(root.path(), &session);
        fs::create_dir_all(&paths.root).context("failed to create session root")?;

        let outcome = self::delete_session_paths(&paths)?;

        pretty_assertions::assert_eq!(outcome, SessionDeleteOutcome::StoppedRemoved);
        assert2::assert!(!paths.root.exists());
        Ok(())
    }

    #[test]
    fn test_delete_session_paths_when_session_is_stopped_keeps_centralized_logs() -> rootcause::Result<()> {
        let root = tempfile::tempdir().context("failed to create muxr delete test root")?;
        let session: SessionName = "dead".parse()?;
        let timestamp = "20260611143012".parse()?;
        let paths = self::test_paths(root.path(), &session);
        fs::create_dir_all(&paths.root).context("failed to create session root")?;
        fs::create_dir_all(paths.logs_root()?).context("failed to create logs root")?;
        let log_path = paths.server_log_path(&session, &timestamp, 12345)?;
        fs::write(&log_path, b"log").context("failed to create session log")?;

        let outcome = self::delete_session_paths(&paths)?;

        pretty_assertions::assert_eq!(outcome, SessionDeleteOutcome::StoppedRemoved);
        assert2::assert!(!paths.root.exists());
        assert2::assert!(log_path.exists());
        Ok(())
    }

    #[test]
    fn test_delete_session_paths_when_session_is_unknown_force_removes_root_and_socket() -> rootcause::Result<()> {
        let root = tempfile::tempdir().context("failed to create muxr delete test root")?;
        let session: SessionName = "dead".parse()?;
        let paths = self::test_paths(root.path(), &session);
        fs::create_dir_all(&paths.root).context("failed to create session root")?;
        let socket_parent = paths.socket.parent().ok_or_else(|| report!("expected socket parent"))?;
        fs::create_dir_all(socket_parent).context("failed to create socket parent")?;
        fs::write(&paths.socket, b"stale").context("failed to create stale socket")?;

        let outcome = self::delete_session_paths(&paths)?;

        pretty_assertions::assert_eq!(outcome, SessionDeleteOutcome::UnknownForced);
        assert2::assert!(!paths.root.exists());
        assert2::assert!(!paths.socket.exists());
        Ok(())
    }

    #[test]
    fn test_delete_session_paths_when_pid_exists_without_socket_force_removes_root() -> rootcause::Result<()> {
        let root = tempfile::tempdir().context("failed to create muxr delete test root")?;
        let session: SessionName = "dead".parse()?;
        let paths = self::test_paths(root.path(), &session);
        fs::create_dir_all(&paths.root).context("failed to create session root")?;
        fs::write(&paths.pid, b"123").context("failed to create stale pid")?;

        let outcome = self::delete_session_paths(&paths)?;

        pretty_assertions::assert_eq!(outcome, SessionDeleteOutcome::UnknownForced);
        assert2::assert!(!paths.root.exists());
        Ok(())
    }

    #[test]
    fn test_delete_session_paths_when_live_session_vanishes_force_removes_files() -> rootcause::Result<()> {
        let root = tempfile::tempdir().context("failed to create muxr delete test root")?;
        let session: SessionName = "vanished".parse()?;
        let paths = self::test_paths(root.path(), &session);
        fs::create_dir_all(&paths.root).context("failed to create session root")?;
        let socket_parent = paths.socket.parent().ok_or_else(|| report!("expected socket parent"))?;
        fs::create_dir_all(socket_parent).context("failed to create socket parent")?;
        let socket = paths.socket.clone();
        let socket_for_server = paths.socket.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let server = std::thread::spawn(move || -> rootcause::Result<()> {
            tokio::runtime::Runtime::new()
                .context("failed to create fake muxr runtime")?
                .block_on(async move {
                    let listener = muxr_transport::ServerListener::bind(&socket_for_server)?;
                    ready_tx.send(()).map_err(|error| {
                        report!("failed to signal fake muxr server readiness").attach(format!("{error}"))
                    })?;
                    let mut connection = listener.accept().await?;
                    drop(listener);
                    self::remove_file_if_exists(&socket)?;
                    pretty_assertions::assert_eq!(connection.recv_request().await?, Some(ClientRequest::Ping));
                    connection.send_event(&ServerEvent::Pong).await
                })
        });
        ready_rx
            .recv_timeout(SESSION_PROBE_TIMEOUT)
            .map_err(|error| report!("timed out waiting for fake muxr server").attach(format!("{error}")))?;

        let outcome = self::delete_session_paths(&paths)?;

        server
            .join()
            .map_err(|error| report!("fake muxr session thread panicked").attach(format!("{error:?}")))??;
        pretty_assertions::assert_eq!(outcome, SessionDeleteOutcome::LiveVanishedForced);
        assert2::assert!(!paths.root.exists());
        assert2::assert!(!paths.socket.exists());
        Ok(())
    }

    fn test_paths(root: &Path, session: &SessionName) -> SessionPaths {
        SessionPaths {
            root: root.join("sessions").join(session.as_ref()),
            socket: root.join("s").join("dead.sock"),
            pid: root.join("sessions").join(session.as_ref()).join("server.pid"),
            layout: root.join("sessions").join(session.as_ref()).join("layout.json"),
            panes: root.join("sessions").join(session.as_ref()).join("panes"),
        }
    }
}
