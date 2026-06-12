use std::path::PathBuf;

use muxr_core::SERVER_LOG_TIMESTAMP_FORMAT;
use muxr_core::ServerLogTimestamp;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use rootcause::prelude::ResultExt;
use tracing::Dispatch;
use tracing_appender::non_blocking::WorkerGuard;

pub mod server {
    use std::path::Path;

    use muxr_core::SessionPaths;

    pub fn starting(paths: &SessionPaths) {
        tracing::info!(
            kind = "server_starting",
            socket_path = %paths.socket.display(),
            pid_path = %paths.pid.display(),
            "muxr server starting"
        );
    }

    pub fn ready(paths: &SessionPaths) {
        tracing::info!(
            kind = "server_ready",
            socket_path = %paths.socket.display(),
            pid_path = %paths.pid.display(),
            "muxr server ready"
        );
    }

    pub fn shutdown(reason: &str) {
        tracing::info!(kind = "server_shutdown", reason = reason, "muxr server shutdown");
    }

    pub fn error(error: &rootcause::Report) {
        tracing::error!(
            kind = "server_error",
            error = %error,
            summary = "muxr server stopped with error",
            "muxr server stopped with error"
        );
    }

    pub fn file_cleanup_failed(event: &str, path: &Path, error: &std::io::Error) {
        tracing::warn!(
            kind = "server_file_cleanup_failed",
            event = event,
            path = %path.display(),
            error = %error,
            summary = "muxr server file cleanup failed",
            "muxr server file cleanup failed"
        );
    }
}

pub mod attached_client {
    pub fn state_handoff_failed(reason: &str) {
        tracing::warn!(
            kind = "attached_client_state_handoff_failed",
            event = "finished_state_send",
            reason = reason,
            summary = "muxr attached client failed to return session state",
            "muxr attached client failed to return session state"
        );
    }
}

pub mod ack {
    use super::ClientEventSendFailure;

    pub fn delete_failed(reason: ClientEventSendFailure) {
        tracing::warn!(
            kind = "delete_ack_send_failed",
            event = "deleted",
            reason = reason.as_str(),
            summary = "muxr delete acknowledgement was not delivered",
            "muxr delete acknowledgement was not delivered"
        );
    }

    pub fn detach_failed(reason: ClientEventSendFailure) {
        tracing::warn!(
            kind = "detach_ack_send_failed",
            event = "detached",
            reason = reason.as_str(),
            summary = "muxr detach acknowledgement was not delivered",
            "muxr detach acknowledgement was not delivered"
        );
    }
}

pub mod scrollback {
    use std::path::Path;

    use muxr_core::PaneId;

    pub fn restore_failed(error: &rootcause::Report) {
        tracing::warn!(
            kind = "scrollback_editor_restore_failed",
            event = "restore_after_client_error",
            error = %error,
            summary = "muxr scrollback editor restore failed while handling client error",
            "muxr scrollback editor restore failed while handling client error"
        );
    }

    pub fn cleanup_failed(event: &str, pane_id: Option<PaneId>, path: &Path, error: &std::io::Error) {
        let Some(pane_id) = pane_id else {
            tracing::warn!(
                kind = "scrollback_cleanup_failed",
                event = event,
                path = %path.display(),
                error = %error,
                summary = "muxr scrollback cleanup failed",
                "muxr scrollback cleanup failed"
            );
            return;
        };
        tracing::warn!(
            pane_id = %pane_id,
            kind = "scrollback_cleanup_failed",
            event = event,
            path = %path.display(),
            error = %error,
            summary = "muxr scrollback cleanup failed",
            "muxr scrollback cleanup failed"
        );
    }
}

pub mod pty {
    pub fn shutdown_failed(event: &str, error: impl std::fmt::Display) {
        tracing::warn!(
            kind = "pty_shutdown_failed",
            event = event,
            error = %error,
            summary = "muxr pty shutdown cleanup failed",
            "muxr pty shutdown cleanup failed"
        );
    }

    pub fn reader_stopped_after_error(event: &str, error: impl std::fmt::Display) {
        tracing::warn!(
            kind = "pty_reader_stopped_after_error",
            event = event,
            error = %error,
            summary = "muxr pty reader stopped after recoverable processing error",
            "muxr pty reader stopped after recoverable processing error"
        );
    }
}

pub struct SessionTracing {
    _guard: WorkerGuard,
    log_path: PathBuf,
}

impl SessionTracing {
    pub fn install_global(paths: &SessionPaths, session: &SessionName) -> rootcause::Result<Self> {
        let log_timestamp = self::current_server_log_timestamp()?;
        let (session_tracing, dispatch) = Self::new(paths, session, &log_timestamp, std::process::id())?;
        tracing::dispatcher::set_global_default(dispatch).context("failed to install muxr server tracing")?;
        tracing::info!(
            session = %session,
            log_path = %session_tracing.log_path.display(),
            "muxr server tracing initialized"
        );
        Ok(session_tracing)
    }

    fn new(
        paths: &SessionPaths,
        session: &SessionName,
        log_timestamp: &ServerLogTimestamp,
        pid: u32,
    ) -> rootcause::Result<(Self, Dispatch)> {
        let log_path = paths.server_log_path(session, log_timestamp, pid)?;
        let file_appender = tracing_appender::rolling::never(
            paths.logs_root()?,
            log_path
                .file_name()
                .ok_or_else(|| rootcause::report!("muxr server log path has no file name"))?,
        );
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        let subscriber = tracing_subscriber::fmt()
            .compact()
            .with_ansi(false)
            .with_target(false)
            .with_writer(non_blocking)
            .finish();

        Ok((
            Self {
                _guard: guard,
                log_path,
            },
            Dispatch::new(subscriber),
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientEventSendFailure {
    SendFailed,
    Timeout,
}

impl ClientEventSendFailure {
    const fn as_str(self) -> &'static str {
        match self {
            Self::SendFailed => "send_failed",
            Self::Timeout => "timeout",
        }
    }
}

#[cfg(test)]
pub fn collect_test_log(
    session: &SessionName,
    action: impl FnOnce() -> rootcause::Result<()>,
) -> rootcause::Result<String> {
    let tempdir = tempfile::tempdir()?;
    let timestamp = "20260611143012".parse()?;
    let paths = SessionPaths::from_sessions_root_path(&tempdir.path().join("sessions"), session)?;
    crate::session_files::prepare_session_dirs(&paths)?;
    let (session_tracing, dispatch) = SessionTracing::new(&paths, session, &timestamp, 12345)?;
    let log_path = session_tracing.log_path.clone();

    let action_result = tracing::dispatcher::with_default(&dispatch, action);
    drop(dispatch);
    drop(session_tracing);
    action_result?;

    match std::fs::read_to_string(&log_path) {
        Ok(log) => Ok(log),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(rootcause::report!("failed to read muxr test tracing log").attach(format!("error={error}"))),
    }
}

fn current_server_log_timestamp() -> rootcause::Result<ServerLogTimestamp> {
    chrono::Local::now()
        .format(SERVER_LOG_TIMESTAMP_FORMAT)
        .to_string()
        .parse()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use muxr_core::PaneId;
    use muxr_core::SessionPaths;

    use super::*;
    use crate::session_files::prepare_session_dirs;

    #[test]
    fn test_session_tracing_when_initialized_uses_centralized_log_path() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let session = "work".parse()?;
        let timestamp = "20260611143012".parse()?;
        let paths = SessionPaths::from_sessions_root_path(&tempdir.path().join("sessions"), &session)?;
        prepare_session_dirs(&paths)?;

        let (session_tracing, dispatch) = SessionTracing::new(&paths, &session, &timestamp, 12345)?;

        pretty_assertions::assert_eq!(
            session_tracing.log_path,
            paths.server_log_path(&session, &timestamp, 12345)?
        );
        // Drop the dispatch before the appender guard so the non-blocking worker is not shut down while the
        // subscriber still owns the writer.
        drop(dispatch);
        drop(session_tracing);
        Ok(())
    }

    #[test]
    fn test_session_tracing_when_event_is_emitted_writes_compact_file_log() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let session: SessionName = "work".parse()?;
        let timestamp = "20260611143012".parse()?;
        let paths = SessionPaths::from_sessions_root_path(&tempdir.path().join("sessions"), &session)?;
        prepare_session_dirs(&paths)?;
        let (session_tracing, dispatch) = SessionTracing::new(&paths, &session, &timestamp, 12345)?;
        let log_path = session_tracing.log_path.clone();

        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!(session = %session, kind = "test_event", "muxr tracing test event");
        });
        drop(dispatch);
        drop(session_tracing);

        let log = fs::read_to_string(&log_path).context("failed to read muxr test tracing log")?;
        assert2::assert!(log_path.exists());
        assert2::assert!(log.contains("muxr tracing test event"));
        assert2::assert!(log.contains("kind=\"test_event\""));
        assert2::assert!(!log.trim_start().starts_with('{'));
        assert2::assert!(!log.contains('\u{1b}'));
        Ok(())
    }

    #[test]
    fn test_session_tracing_when_server_events_are_emitted_writes_stable_fields() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let session: SessionName = "work".parse()?;
        let timestamp = "20260611143012".parse()?;
        let paths = SessionPaths::from_sessions_root_path(&tempdir.path().join("sessions"), &session)?;
        prepare_session_dirs(&paths)?;
        let (session_tracing, dispatch) = SessionTracing::new(&paths, &session, &timestamp, 12345)?;
        let log_path = session_tracing.log_path.clone();
        let pane_id = PaneId::new(7)?;

        tracing::dispatcher::with_default(&dispatch, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            server::ready(&paths);
            server::shutdown("final_pane_exited");
            server::error(&rootcause::report!("test server failure"));
            attached_client::state_handoff_failed("channel_full");
            scrollback::restore_failed(&rootcause::report!("test restore failure"));
            ack::delete_failed(ClientEventSendFailure::Timeout);
            ack::detach_failed(ClientEventSendFailure::SendFailed);
            server::file_cleanup_failed(
                "remove_socket",
                Path::new("/tmp/muxr.sock"),
                &std::io::Error::other("socket busy"),
            );
            scrollback::cleanup_failed(
                "remove_editor_history",
                Some(pane_id),
                Path::new("/tmp/muxr/panes/7"),
                &std::io::Error::other("history busy"),
            );
            scrollback::cleanup_failed(
                "remove_dump_after_error",
                None,
                Path::new("/tmp/muxr/scrollback/dump.txt"),
                &std::io::Error::other("dump busy"),
            );
            pty::shutdown_failed("kill_child", std::io::Error::other("kill busy"));
            pty::reader_stopped_after_error("write_terminal_replies", rootcause::report!("reply failed"));
        });
        drop(dispatch);
        drop(session_tracing);

        let log = fs::read_to_string(&log_path).context("failed to read muxr test tracing log")?;
        assert2::assert!(log.contains("session=work"));
        assert2::assert!(log.contains("kind=\"server_ready\""));
        assert2::assert!(log.contains("kind=\"server_shutdown\""));
        assert2::assert!(log.contains("reason=\"final_pane_exited\""));
        assert2::assert!(log.contains("kind=\"server_error\""));
        assert2::assert!(log.contains("summary=\"muxr server stopped with error\""));
        assert2::assert!(log.contains("test server failure"));
        assert2::assert!(log.contains("kind=\"attached_client_state_handoff_failed\""));
        assert2::assert!(log.contains("event=\"finished_state_send\""));
        assert2::assert!(log.contains("reason=\"channel_full\""));
        assert2::assert!(log.contains("kind=\"scrollback_editor_restore_failed\""));
        assert2::assert!(log.contains("event=\"restore_after_client_error\""));
        assert2::assert!(log.contains("test restore failure"));
        assert2::assert!(log.contains("kind=\"delete_ack_send_failed\""));
        assert2::assert!(log.contains("event=\"deleted\""));
        assert2::assert!(log.contains("reason=\"timeout\""));
        assert2::assert!(log.contains("kind=\"detach_ack_send_failed\""));
        assert2::assert!(log.contains("event=\"detached\""));
        assert2::assert!(log.contains("reason=\"send_failed\""));
        assert2::assert!(log.contains("kind=\"server_file_cleanup_failed\""));
        assert2::assert!(log.contains("event=\"remove_socket\""));
        assert2::assert!(log.contains("path=/tmp/muxr.sock"));
        assert2::assert!(log.contains("socket busy"));
        assert2::assert!(log.contains("kind=\"scrollback_cleanup_failed\""));
        assert2::assert!(log.contains("event=\"remove_editor_history\""));
        assert2::assert!(log.contains("pane_id=pane-7"));
        assert2::assert!(log.contains("history busy"));
        assert2::assert!(log.contains("event=\"remove_dump_after_error\""));
        assert2::assert!(log.contains("dump busy"));
        assert2::assert!(log.contains("kind=\"pty_shutdown_failed\""));
        assert2::assert!(log.contains("event=\"kill_child\""));
        assert2::assert!(log.contains("summary=\"muxr pty shutdown cleanup failed\""));
        assert2::assert!(log.contains("kill busy"));
        assert2::assert!(log.contains("kind=\"pty_reader_stopped_after_error\""));
        assert2::assert!(log.contains("event=\"write_terminal_replies\""));
        assert2::assert!(log.contains("summary=\"muxr pty reader stopped after recoverable processing error\""));
        assert2::assert!(log.contains("reply failed"));
        Ok(())
    }
}
