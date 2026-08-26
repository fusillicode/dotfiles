use std::time::Duration;

use muxr_core::ServerEvent;

use crate::render_state::ClientEventSendOutcome;
use crate::render_worker::RenderInput;
use crate::session::tracing::ClientEventSendFailure;

pub type HeartbeatDelivery = tokio::sync::oneshot::Receiver<Result<tokio::time::Instant, ClientEventSendFailure>>;

pub trait ServerEventSink {
    async fn send_event(&mut self, event: &ServerEvent) -> rootcause::Result<()>;

    async fn send_event_and_render(&mut self, event: &ServerEvent, input: &RenderInput) -> rootcause::Result<()>;

    async fn send_lifecycle_event(&mut self, event: &ServerEvent) -> rootcause::Result<()>;

    async fn send_heartbeat(&mut self) -> rootcause::Result<HeartbeatDelivery>;

    async fn send_render(&mut self, input: &RenderInput) -> rootcause::Result<()>;
}

pub async fn send_heartbeat_with_timeout(
    writer: &mut impl ServerEventSink,
    client_write_timeout: Duration,
) -> Result<HeartbeatDelivery, ClientEventSendFailure> {
    match tokio::time::timeout(client_write_timeout, writer.send_heartbeat()).await {
        Ok(Ok(delivery)) => Ok(delivery),
        Ok(Err(_)) => Err(ClientEventSendFailure::SendFailed),
        Err(_) => Err(ClientEventSendFailure::Timeout),
    }
}

pub async fn send_event_and_render_with_timeout(
    writer: &mut impl ServerEventSink,
    event: &ServerEvent,
    input: &RenderInput,
    client_write_timeout: Duration,
) -> rootcause::Result<ClientEventSendOutcome> {
    Ok(
        tokio::time::timeout(client_write_timeout, writer.send_event_and_render(event, input))
            .await
            .map_or(
                ClientEventSendOutcome::Failed(crate::session::tracing::ClientEventSendFailure::Timeout),
                |result| {
                    result.map_or(
                        ClientEventSendOutcome::Failed(crate::session::tracing::ClientEventSendFailure::SendFailed),
                        |()| ClientEventSendOutcome::Sent,
                    )
                },
            ),
    )
}

pub async fn send_render_with_timeout(
    writer: &mut impl ServerEventSink,
    input: &RenderInput,
    client_write_timeout: Duration,
) -> rootcause::Result<ClientEventSendOutcome> {
    Ok(tokio::time::timeout(client_write_timeout, writer.send_render(input))
        .await
        .map_or(
            ClientEventSendOutcome::Failed(crate::session::tracing::ClientEventSendFailure::Timeout),
            |result| {
                result.map_or(
                    ClientEventSendOutcome::Failed(crate::session::tracing::ClientEventSendFailure::SendFailed),
                    |()| ClientEventSendOutcome::Sent,
                )
            },
        ))
}

/// Send one event on an attached-client writer with the server's bounded write timeout.
///
/// # Errors
/// Transport errors and write timeouts are represented as `ClientEventSendOutcome::Failed` so attached-client loop
/// callers handle expected disconnects separately from local processing errors.
pub async fn send_event_with_timeout(
    writer: &mut impl ServerEventSink,
    event: &ServerEvent,
    client_write_timeout: Duration,
) -> rootcause::Result<ClientEventSendOutcome> {
    Ok(self::send_event_failure(writer, event, client_write_timeout)
        .await
        .map_or(ClientEventSendOutcome::Sent, ClientEventSendOutcome::Failed))
}

pub async fn send_event_failure(
    writer: &mut impl ServerEventSink,
    event: &ServerEvent,
    client_write_timeout: Duration,
) -> Option<ClientEventSendFailure> {
    match tokio::time::timeout(client_write_timeout, writer.send_event(event)).await {
        Ok(Ok(())) => None,
        Ok(Err(_)) => Some(ClientEventSendFailure::SendFailed),
        Err(_) => Some(ClientEventSendFailure::Timeout),
    }
}

pub async fn send_lifecycle_event_failure(
    writer: &mut impl ServerEventSink,
    event: &ServerEvent,
    client_write_timeout: Duration,
) -> Option<ClientEventSendFailure> {
    match tokio::time::timeout(client_write_timeout, writer.send_lifecycle_event(event)).await {
        Ok(Ok(())) => None,
        Ok(Err(_)) => Some(ClientEventSendFailure::SendFailed),
        Err(_) => Some(ClientEventSendFailure::Timeout),
    }
}
