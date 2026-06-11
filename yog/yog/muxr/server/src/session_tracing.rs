use std::path::PathBuf;

use muxr_core::SERVER_LOG_TIMESTAMP_FORMAT;
use muxr_core::ServerLogTimestamp;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use rootcause::prelude::ResultExt;
use tracing::Dispatch;
use tracing_appender::non_blocking::WorkerGuard;

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

fn current_server_log_timestamp() -> rootcause::Result<ServerLogTimestamp> {
    chrono::Local::now()
        .format(SERVER_LOG_TIMESTAMP_FORMAT)
        .to_string()
        .parse()
}

pub fn record_server_starting(session: &SessionName, paths: &SessionPaths) {
    tracing::info!(
        session = %session,
        kind = "server_starting",
        socket_path = %paths.socket.display(),
        pid_path = %paths.pid.display(),
        "muxr server starting"
    );
}

pub fn record_server_ready(session: &SessionName, paths: &SessionPaths) {
    tracing::info!(
        session = %session,
        kind = "server_ready",
        socket_path = %paths.socket.display(),
        pid_path = %paths.pid.display(),
        "muxr server ready"
    );
}

pub fn record_server_shutdown(session: &SessionName, reason: &str) {
    tracing::info!(
        session = %session,
        kind = "server_shutdown",
        reason = reason,
        "muxr server shutdown"
    );
}

pub fn record_server_error(session: &SessionName, error: &rootcause::Report) {
    tracing::error!(
        session = %session,
        kind = "server_error",
        error = %error,
        summary = "muxr server stopped with error",
        "muxr server stopped with error"
    );
}

#[cfg(test)]
mod tests {
    use std::fs;

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

        tracing::dispatcher::with_default(&dispatch, || {
            record_server_ready(&session, &paths);
            record_server_shutdown(&session, "final_pane_exited");
            record_server_error(&session, &rootcause::report!("test server failure"));
        });
        drop(dispatch);
        drop(session_tracing);

        let log = fs::read_to_string(&log_path).context("failed to read muxr test tracing log")?;
        assert2::assert!(log.contains("kind=\"server_ready\""));
        assert2::assert!(log.contains("kind=\"server_shutdown\""));
        assert2::assert!(log.contains("reason=\"final_pane_exited\""));
        assert2::assert!(log.contains("kind=\"server_error\""));
        assert2::assert!(log.contains("summary=\"muxr server stopped with error\""));
        assert2::assert!(log.contains("test server failure"));
        Ok(())
    }
}
