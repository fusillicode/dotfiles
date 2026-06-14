use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use muxr_core::ServerEvent;
use muxr_core::SessionPaths;
use muxr_transport::ServerConnection;
use muxr_transport::ServerEventWriter;
use rootcause::prelude::ResultExt;

use crate::session::tracing::ClientEventSendFailure;

#[derive(Debug, Default)]
pub struct DeleteSessions {
    requested: AtomicBool,
}

impl DeleteSessions {
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }
}

pub async fn handle_handshake_delete(
    connection: &mut ServerConnection,
    delete_sessions: &DeleteSessions,
    client_write_timeout: Duration,
) -> rootcause::Result<()> {
    delete_sessions.request();
    self::record_delete_ack_send_failure(
        self::send_connection_event_failure(connection, &ServerEvent::Deleted, client_write_timeout).await,
    );
    Ok(())
}

pub async fn handle_client_delete(
    event_writer: &mut ServerEventWriter,
    delete_sessions: &DeleteSessions,
    client_write_timeout: Duration,
) -> rootcause::Result<()> {
    delete_sessions.request();
    self::record_delete_ack_send_failure(
        crate::event_writer::send_event_failure(event_writer, &ServerEvent::Deleted, client_write_timeout).await,
    );
    Ok(())
}

pub fn remove_session_files(paths: &SessionPaths) -> rootcause::Result<()> {
    // Live deletion is server-owned so pane processes and history writers are dropped before state removal.
    match fs::remove_dir_all(&paths.root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to remove muxr session dir")?,
    }
    match fs::remove_file(&paths.socket) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to remove muxr session socket")?,
    }
    Ok(())
}

async fn send_connection_event_failure(
    connection: &mut ServerConnection,
    event: &ServerEvent,
    client_write_timeout: Duration,
) -> Option<ClientEventSendFailure> {
    match tokio::time::timeout(client_write_timeout, connection.send_event(event)).await {
        Ok(Ok(())) => None,
        Ok(Err(_)) => Some(ClientEventSendFailure::SendFailed),
        Err(_) => Some(ClientEventSendFailure::Timeout),
    }
}

fn record_delete_ack_send_failure(reason: Option<ClientEventSendFailure>) {
    if let Some(reason) = reason {
        crate::session::tracing::ack::delete_failed(reason);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use muxr_core::SessionName;
    use rootcause::prelude::ResultExt;

    use super::*;

    #[test]
    fn test_remove_session_files_keeps_centralized_logs() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let session: SessionName = "work".parse()?;
        let timestamp = "20260611143012".parse()?;
        let paths = self::test_paths(tempdir.path(), &session);
        fs::create_dir_all(&paths.root).context("failed to create test session root")?;
        fs::create_dir_all(
            paths
                .socket
                .parent()
                .ok_or_else(|| rootcause::report!("expected socket parent"))?,
        )
        .context("failed to create test socket root")?;
        fs::write(&paths.socket, b"socket").context("failed to create test socket")?;
        fs::create_dir_all(paths.logs_root()?).context("failed to create test logs root")?;
        let log_path = paths.server_log_path(&session, &timestamp, 12345)?;
        fs::write(&log_path, b"log").context("failed to create test log")?;

        remove_session_files(&paths)?;

        assert2::assert!(!paths.root.exists());
        assert2::assert!(!paths.socket.exists());
        assert2::assert!(log_path.exists());
        Ok(())
    }

    #[test]
    fn test_record_delete_ack_send_failure_when_reason_exists_warns() -> rootcause::Result<()> {
        let session = SessionName::default();

        let log = crate::session::tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            self::record_delete_ack_send_failure(Some(ClientEventSendFailure::Timeout));
            Ok(())
        })?;
        assert2::assert!(log.contains("kind=\"delete_ack_send_failed\""));
        assert2::assert!(log.contains("event=\"deleted\""));
        assert2::assert!(log.contains("reason=\"timeout\""));
        Ok(())
    }

    #[test]
    fn test_record_delete_ack_send_failure_when_reason_is_none_is_silent() -> rootcause::Result<()> {
        let session = SessionName::default();
        let log = crate::session::tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            self::record_delete_ack_send_failure(None);
            Ok(())
        })?;

        assert2::assert!(!log.contains("kind=\"delete_ack_send_failed\""));
        Ok(())
    }

    fn test_paths(root: &Path, session: &SessionName) -> SessionPaths {
        SessionPaths {
            root: root.join("sessions").join(session.as_ref()),
            socket: root.join("s").join("work.sock"),
            pid: root.join("sessions").join(session.as_ref()).join("server.pid"),
            layout: root.join("sessions").join(session.as_ref()).join("layout.json"),
            panes: root.join("sessions").join(session.as_ref()).join("panes"),
        }
    }
}
