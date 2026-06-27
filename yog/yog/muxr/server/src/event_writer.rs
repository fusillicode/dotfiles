use std::time::Duration;

use muxr_core::ServerEvent;
use muxr_transport::ServerEventWriter;

use crate::render_state::ClientEventSendOutcome;
use crate::session::tracing::ClientEventSendFailure;

/// Send one event on an attached-client writer with the server's bounded write timeout.
///
/// # Errors
/// Transport errors and write timeouts are represented as `ClientEventSendOutcome::Failed` so attached-client loop
/// callers handle expected disconnects separately from local processing errors.
pub async fn send_event_with_timeout(
    writer: &mut ServerEventWriter,
    event: &ServerEvent,
    client_write_timeout: Duration,
) -> rootcause::Result<ClientEventSendOutcome> {
    Ok(self::send_event_failure(writer, event, client_write_timeout)
        .await
        .map_or(ClientEventSendOutcome::Sent, ClientEventSendOutcome::Failed))
}

pub async fn send_event_failure(
    writer: &mut ServerEventWriter,
    event: &ServerEvent,
    client_write_timeout: Duration,
) -> Option<ClientEventSendFailure> {
    match tokio::time::timeout(client_write_timeout, writer.send_event(event)).await {
        Ok(Ok(())) => None,
        Ok(Err(_)) => Some(ClientEventSendFailure::SendFailed),
        Err(_) => Some(ClientEventSendFailure::Timeout),
    }
}
