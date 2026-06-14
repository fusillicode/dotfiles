use std::time::Duration;

use crate::session::delete::DeleteSessions;
use crate::session::tracing::ClientEventSendFailure;

pub fn client_should_exit(
    output_current: impl IntoIterator<Item = bool>,
    client_heartbeat_timeout: Duration,
    delete_sessions: &DeleteSessions,
    heartbeat_started_at: Option<tokio::time::Instant>,
) -> bool {
    // A dropped PTY sink means live output is already stale; release the active slot instead of draining old frames
    // into a slow client.
    if output_current.into_iter().any(|is_current| !is_current) {
        return true;
    }
    if let Some(started_at) = heartbeat_started_at
        && started_at.elapsed() > client_heartbeat_timeout
    {
        return true;
    }
    // The delete requester already received the explicit ack; attached clients can observe connection close. Waiting
    // to notify a slow attached terminal would delay server-owned cleanup of the selected session.
    delete_sessions.is_requested()
}

pub fn record_detach_ack_send_failure(reason: Option<ClientEventSendFailure>) {
    if let Some(reason) = reason {
        crate::session::tracing::ack::detach_failed(reason);
    }
}

#[cfg(test)]
mod tests {
    use muxr_core::SessionName;

    use super::*;

    #[test]
    fn test_client_should_exit_when_everything_is_current_returns_false() {
        let delete_sessions = DeleteSessions::default();

        assert2::assert!(!client_should_exit(
            [true, true],
            Duration::from_secs(1),
            &delete_sessions,
            None,
        ));
    }

    #[test]
    fn test_client_should_exit_when_output_is_stale_returns_true() {
        let delete_sessions = DeleteSessions::default();

        assert2::assert!(client_should_exit(
            [true, false],
            Duration::from_secs(1),
            &delete_sessions,
            None,
        ));
    }

    #[test]
    fn test_client_should_exit_when_heartbeat_timed_out_returns_true() {
        let delete_sessions = DeleteSessions::default();
        let heartbeat_started_at = tokio::time::Instant::now() - Duration::from_secs(2);

        assert2::assert!(client_should_exit(
            [true],
            Duration::from_secs(1),
            &delete_sessions,
            Some(heartbeat_started_at),
        ));
    }

    #[test]
    fn test_client_should_exit_when_delete_is_requested_returns_true() {
        let delete_sessions = DeleteSessions::default();
        delete_sessions.request();

        assert2::assert!(client_should_exit(
            [true],
            Duration::from_secs(1),
            &delete_sessions,
            None,
        ));
    }

    #[test]
    fn test_record_detach_ack_send_failure_when_reason_exists_warns() -> rootcause::Result<()> {
        let session = SessionName::default();

        let log = crate::session::tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            record_detach_ack_send_failure(Some(ClientEventSendFailure::SendFailed));
            Ok(())
        })?;

        assert2::assert!(log.contains("kind=\"detach_ack_send_failed\""));
        assert2::assert!(log.contains("event=\"detached\""));
        assert2::assert!(log.contains("reason=\"send_failed\""));
        Ok(())
    }

    #[test]
    fn test_record_detach_ack_send_failure_when_reason_is_none_is_silent() -> rootcause::Result<()> {
        let session = SessionName::default();
        let log = crate::session::tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            record_detach_ack_send_failure(None);
            Ok(())
        })?;

        assert2::assert!(!log.contains("kind=\"detach_ack_send_failed\""));
        Ok(())
    }
}
