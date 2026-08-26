use std::time::Duration;

use crate::event_writer::HeartbeatDelivery;
use crate::event_writer::ServerEventSink;
use crate::render_state::ClientSessionFlow;
use crate::session::tracing::ClientEventSendFailure;

pub struct HeartbeatTracker {
    phase: HeartbeatPhase,
}

pub trait HeartbeatStatus {
    fn acknowledge(&mut self);
    fn response_started_at(&self) -> Option<tokio::time::Instant>;
    fn is_idle(&self) -> bool;
    fn track(&mut self, delivery: HeartbeatDelivery);
    fn sync_delivery(&mut self) -> ClientSessionFlow;
}

enum HeartbeatPhase {
    Idle,
    Queued(tokio::sync::oneshot::Receiver<Result<tokio::time::Instant, ClientEventSendFailure>>),
    AwaitingPong(tokio::time::Instant),
}

impl Default for HeartbeatTracker {
    fn default() -> Self {
        Self {
            phase: HeartbeatPhase::Idle,
        }
    }
}

impl HeartbeatStatus for HeartbeatTracker {
    fn acknowledge(&mut self) {
        self.phase = HeartbeatPhase::Idle;
    }

    fn response_started_at(&self) -> Option<tokio::time::Instant> {
        match self.phase {
            HeartbeatPhase::AwaitingPong(started_at) => Some(started_at),
            HeartbeatPhase::Idle | HeartbeatPhase::Queued(_) => None,
        }
    }

    fn is_idle(&self) -> bool {
        matches!(self.phase, HeartbeatPhase::Idle)
    }

    fn track(&mut self, delivery: HeartbeatDelivery) {
        self.phase = HeartbeatPhase::Queued(delivery);
    }

    fn sync_delivery(&mut self) -> ClientSessionFlow {
        let HeartbeatPhase::Queued(delivery) = &mut self.phase else {
            return ClientSessionFlow::Continue;
        };
        match delivery.try_recv() {
            Ok(Ok(delivered_at)) => {
                self.phase = HeartbeatPhase::AwaitingPong(delivered_at);
                ClientSessionFlow::Continue
            }
            Ok(Err(_)) | Err(tokio::sync::oneshot::error::TryRecvError::Closed) => ClientSessionFlow::Disconnect,
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => ClientSessionFlow::Continue,
        }
    }
}

#[cfg(test)]
impl HeartbeatStatus for Option<tokio::time::Instant> {
    fn acknowledge(&mut self) {
        *self = None;
    }

    fn response_started_at(&self) -> Option<tokio::time::Instant> {
        *self
    }

    fn is_idle(&self) -> bool {
        self.is_none()
    }

    fn track(&mut self, delivery: HeartbeatDelivery) {
        drop(delivery);
        *self = Some(tokio::time::Instant::now());
    }

    fn sync_delivery(&mut self) -> ClientSessionFlow {
        ClientSessionFlow::Continue
    }
}

pub async fn send_if_idle(
    event_writer: &mut impl ServerEventSink,
    client_write_timeout: Duration,
    heartbeat: &mut impl HeartbeatStatus,
) -> rootcause::Result<ClientSessionFlow> {
    if !heartbeat.is_idle() {
        return Ok(ClientSessionFlow::Continue);
    }

    match crate::event_writer::send_heartbeat_with_timeout(event_writer, client_write_timeout).await {
        Ok(delivery) => heartbeat.track(delivery),
        Err(_) => return Ok(ClientSessionFlow::Disconnect),
    }
    Ok(ClientSessionFlow::Continue)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rootcause::report;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_heartbeat_timeout_starts_at_worker_delivery() -> rootcause::Result<()> {
        let (delivery_sender, delivery_receiver) = tokio::sync::oneshot::channel();
        let mut heartbeat = HeartbeatTracker::default();
        heartbeat.track(delivery_receiver);
        let delivered_at = tokio::time::Instant::now() + Duration::from_secs(30);

        assert_that!(heartbeat.response_started_at(), eq(None));
        assert_that!(heartbeat.sync_delivery(), eq(ClientSessionFlow::Continue));
        assert_that!(heartbeat.response_started_at(), eq(None));

        delivery_sender
            .send(Ok(delivered_at))
            .map_err(|_| report!("heartbeat delivery receiver unexpectedly dropped"))?;
        assert_that!(heartbeat.sync_delivery(), eq(ClientSessionFlow::Continue));
        assert_that!(heartbeat.response_started_at(), eq(Some(delivered_at)));
        Ok(())
    }

    #[test]
    fn test_heartbeat_worker_delivery_failure_disconnects_session() {
        let (delivery_sender, delivery_receiver) = tokio::sync::oneshot::channel();
        let mut heartbeat = HeartbeatTracker::default();
        heartbeat.track(delivery_receiver);
        assert_that!(delivery_sender.send(Err(ClientEventSendFailure::Timeout)), ok(eq(())));

        assert_that!(heartbeat.sync_delivery(), eq(ClientSessionFlow::Disconnect));
    }
}
