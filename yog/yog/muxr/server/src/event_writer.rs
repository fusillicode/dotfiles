use std::time::Duration;

use muxr_core::ServerEvent;
use muxr_transport::ServerEventWriter;

use crate::render_state::ClientEventSendOutcome;
use crate::render_worker::ServerRenderCommand;
use crate::session::tracing::ClientEventSendFailure;

pub enum HeartbeatDelivery {
    Delivered(tokio::time::Instant),
    Queued(tokio::sync::oneshot::Receiver<Result<tokio::time::Instant, ClientEventSendFailure>>),
}

// NOTE: The live attached session sends through `RenderWorkerSender`, which keeps the transport writer owned by the
// render worker. This trait preserves direct `ServerEventWriter` use in focused tests and non-worker call paths.
pub trait ServerEventSink {
    async fn send_event(&mut self, event: &ServerEvent) -> rootcause::Result<()>;

    async fn send_event_and_render(
        &mut self,
        event: &ServerEvent,
        command: &ServerRenderCommand,
    ) -> rootcause::Result<()> {
        self.send_event(event).await?;
        self.send_render(command).await
    }

    async fn send_lifecycle_event(&mut self, event: &ServerEvent) -> rootcause::Result<()> {
        self.send_event(event).await
    }

    async fn send_heartbeat(&mut self) -> rootcause::Result<HeartbeatDelivery> {
        self.send_event(&ServerEvent::Ping).await?;
        Ok(HeartbeatDelivery::Delivered(tokio::time::Instant::now()))
    }

    async fn send_render(&mut self, command: &ServerRenderCommand) -> rootcause::Result<()> {
        let ServerRenderCommand::Ready { pane_regions, render } = command else {
            return Err(rootcause::report!(
                "muxr direct event writer received uncomposed render inputs"
            ));
        };
        self.send_event(&ServerEvent::PaneRegions(pane_regions.clone())).await?;
        if let Some(render) = render {
            self.send_event(&ServerEvent::Render(render.clone())).await?;
        }
        Ok(())
    }
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
    command: &ServerRenderCommand,
    client_write_timeout: Duration,
) -> rootcause::Result<ClientEventSendOutcome> {
    Ok(
        tokio::time::timeout(client_write_timeout, writer.send_event_and_render(event, command))
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
    command: &ServerRenderCommand,
    client_write_timeout: Duration,
) -> rootcause::Result<ClientEventSendOutcome> {
    Ok(tokio::time::timeout(client_write_timeout, writer.send_render(command))
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

impl ServerEventSink for ServerEventWriter {
    async fn send_event(&mut self, event: &ServerEvent) -> rootcause::Result<()> {
        self.send_event(event).await
    }
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
