use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use muxr_core::PaneId;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::RenderUpdate;
use muxr_core::ServerEvent;
use muxr_core::TerminalSize;
use muxr_transport::ServerEventWriter;
use tokio::sync::Mutex;
use tokio::sync::Notify;

use crate::event_writer::HeartbeatDelivery;
use crate::event_writer::ServerEventSink;
use crate::pane::layout::PaneLayout;
use crate::pane::render::PaneRenderConfig;
use crate::pane::render::PaneRenderLayout;
use crate::pane::render::RenderComposer;
use crate::pty::PtyHandle;
use crate::render_state::ClientRenderDmg;
use crate::session::tracing::ClientEventSendFailure;

const MANDATORY_EVENT_LIMIT: usize = 128;

/// Compact render intent published by the attached-session actor.
///
/// The worker alone snapshots terminal state, composes, and writes output, so the session actor can return to
/// requests without copying visible terminal grids.
#[derive(Clone)]
pub struct RenderInput {
    active_pane: PaneId,
    attention_panes: Vec<PaneId>,
    damage: ClientRenderDmg,
    force_baseline: bool,
    generation: u64,
    pane_layout: Arc<PaneLayout>,
    pane_render: PaneRenderConfig,
    pane_handles: BTreeMap<PaneId, PtyHandle>,
    size: TerminalSize,
}

impl std::fmt::Debug for RenderInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RenderInput")
            .field("active_pane", &self.active_pane)
            .field("attention_panes", &self.attention_panes)
            .field("damage", &self.damage)
            .field("force_baseline", &self.force_baseline)
            .field("generation", &self.generation)
            .field("pane_layout", &self.pane_layout)
            .field("pane_render", &self.pane_render)
            .field("pane_handle_count", &self.pane_handles.len())
            .field("size", &self.size)
            .finish()
    }
}

impl RenderInput {
    #[expect(
        clippy::too_many_arguments,
        reason = "the command explicitly owns every render input"
    )]
    pub fn new(
        generation: u64,
        pane_render: PaneRenderConfig,
        active_pane: PaneId,
        pane_layout: PaneLayout,
        pane_handles: BTreeMap<PaneId, PtyHandle>,
        size: TerminalSize,
        attention_panes: Vec<PaneId>,
        damage: ClientRenderDmg,
        force_baseline: bool,
    ) -> Self {
        Self {
            active_pane,
            attention_panes,
            damage,
            force_baseline,
            generation,
            pane_layout: Arc::new(pane_layout),
            pane_render,
            pane_handles,
            size,
        }
    }

    pub fn pane_regions(&self) -> rootcause::Result<PaneRegionsSnapshot> {
        let regions = self
            .pane_layout
            .regions()
            .iter()
            .map(|region| {
                let metadata = self
                    .pane_handles
                    .get(&region.id)
                    .ok_or_else(|| {
                        rootcause::report!("muxr render command is missing a pane handle")
                            .attach(format!("pane={}", region.id))
                    })?
                    .pane_render_metadata()?;
                PaneRegionSnapshot::new(
                    region.id,
                    region.area.origin.col,
                    region.area.origin.row,
                    region.area.size.cols,
                    region.area.size.rows,
                    metadata.mouse_mode(),
                    metadata.visible_top_row(),
                )
                .and_then(|region| region.with_wrapped_rows(metadata.visible_row_wraps().to_vec()))
            })
            .collect::<rootcause::Result<Vec<_>>>()?;
        PaneRegionsSnapshot::new(regions)
    }

    fn pane_render_snapshot(&self, pane_id: PaneId) -> rootcause::Result<crate::pty::PtyRenderSnapshot> {
        self.pane_handles
            .get(&pane_id)
            .ok_or_else(|| {
                rootcause::report!("muxr render command is missing a pane handle").attach(format!("pane={pane_id}"))
            })?
            .pane_render_snapshot()
    }

    fn with_complete_damage(mut self) -> Self {
        // Compare the complete newest state with the compositor's last sent frame so no earlier pane damage is lost
        // with the discarded input.
        self.damage = ClientRenderDmg::Full;
        self
    }
}

/// A render command is either immutable work for the live worker or a precomposed update used by focused direct-writer
/// tests. The production `Attached` variant never contains a `RenderComposer`.
pub enum ServerRenderCommand {
    Inputs(RenderInput),
    Ready {
        pane_regions: PaneRegionsSnapshot,
        render: Option<RenderUpdate>,
    },
}

enum RenderWorkerMode {
    Detached(RenderComposer),
    Attached {
        sender: RenderWorkerSender,
        task: Option<tokio::task::JoinHandle<()>>,
    },
}

/// Render coordinator stored by attached-client state.
///
/// Before a transport is attached, the detached mode preserves the repository's direct-writer test seam. In the live
/// path `attach_writer` drops that local composer, spawns the sole render/output owner, and leaves only a sender here.
pub struct RenderWorker {
    generation: u64,
    mode: RenderWorkerMode,
}

impl Default for RenderWorker {
    fn default() -> Self {
        Self {
            generation: 0,
            mode: RenderWorkerMode::Detached(RenderComposer::default()),
        }
    }
}

#[derive(Clone)]
pub struct RenderWorkerSender {
    output: Arc<OutputShared>,
}

struct OutputShared {
    closed: AtomicBool,
    state: Mutex<OutputState>,
    wake: Notify,
}

struct OutputState {
    failure: Option<String>,
    queue: ServerOutputQueue,
}

impl RenderWorker {
    pub const fn stage_mandatory(event: ServerEvent) -> ServerEvent {
        event
    }

    pub fn next_generation(&mut self) -> rootcause::Result<u64> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| rootcause::report!("muxr render generation overflowed"))?;
        Ok(self.generation)
    }

    pub fn attach_writer(
        &mut self,
        writer: ServerEventWriter,
        write_timeout: Duration,
    ) -> rootcause::Result<RenderWorkerSender> {
        if matches!(self.mode, RenderWorkerMode::Attached { .. }) {
            return Err(rootcause::report!("muxr render worker already owns an event writer"));
        }
        let output = Arc::new(OutputShared {
            closed: AtomicBool::new(false),
            state: Mutex::new(OutputState {
                failure: None,
                queue: ServerOutputQueue::new(MANDATORY_EVENT_LIMIT),
            }),
            wake: Notify::new(),
        });
        let sender = RenderWorkerSender {
            output: Arc::clone(&output),
        };
        let task = tokio::spawn(async move {
            Self::run_output(RenderComposer::default(), writer, output, write_timeout).await;
        });
        self.mode = RenderWorkerMode::Attached {
            sender: sender.clone(),
            task: Some(task),
        };
        Ok(sender)
    }

    pub fn stage_render(&mut self, input: RenderInput) -> rootcause::Result<ServerRenderCommand> {
        match &mut self.mode {
            RenderWorkerMode::Attached { .. } => Ok(ServerRenderCommand::Inputs(input)),
            RenderWorkerMode::Detached(composer) => {
                let pane_regions = input.pane_regions()?;
                let render = Self::compose(composer, &input)?;
                Ok(ServerRenderCommand::Ready { pane_regions, render })
            }
        }
    }

    async fn run_output(
        mut composer: RenderComposer,
        mut writer: ServerEventWriter,
        output: Arc<OutputShared>,
        write_timeout: Duration,
    ) {
        let mut pane_regions = None;
        loop {
            let command = loop {
                let notified = output.wake.notified();
                let mut state = output.state.lock().await;
                let closed = output.closed.load(Ordering::Acquire);
                let command = if closed {
                    state.queue.mandatory.pop_front().map(ServerOutputCommand::Mandatory)
                } else {
                    state.queue.pop()
                };
                if let Some(command) = command {
                    break command;
                }
                if closed {
                    return;
                }
                drop(state);
                notified.await;
            };
            let result = match command {
                ServerOutputCommand::Mandatory(MandatoryOutput {
                    event,
                    lifecycle,
                    delivered,
                }) => {
                    let write_result = Self::write_event(&mut writer, &event, write_timeout).await;
                    if let Some(delivered) = delivered {
                        let delivery = write_result
                            .as_ref()
                            .map(|()| tokio::time::Instant::now())
                            .map_err(|failure| failure.reason);
                        let _sent = delivered.send(delivery);
                    }
                    if let (Some(lifecycle), Err(failure)) = (lifecycle, &write_result) {
                        lifecycle.record(failure.reason);
                    }
                    if write_result.is_ok()
                        && let ServerEvent::Attached(attached) = &event
                    {
                        pane_regions = Some(attached.pane_regions.clone());
                    }
                    write_result.map_err(WorkerCommandFailure::from_write)
                }
                ServerOutputCommand::Render(input) => {
                    async {
                        let next_regions = input
                            .pane_regions()
                            .map_err(|error| WorkerCommandFailure::from_local(&error))?;
                        let force_regions = input.force_baseline && composer.has_baseline();
                        let render = Self::compose(&mut composer, &input)
                            .map_err(|error| WorkerCommandFailure::from_local(&error))?;
                        if force_regions || pane_regions.as_ref() != Some(&next_regions) {
                            Self::write_event(
                                &mut writer,
                                &ServerEvent::PaneRegions(next_regions.clone()),
                                write_timeout,
                            )
                            .await
                            .map_err(WorkerCommandFailure::from_write)?;
                            pane_regions = Some(next_regions);
                        }
                        if let Some(render) = render {
                            Self::write_event(&mut writer, &ServerEvent::Render(render), write_timeout)
                                .await
                                .map_err(WorkerCommandFailure::from_write)?;
                        }
                        Ok(())
                    }
                    .await
                }
            };
            if let Err(failure) = result {
                let reason = failure.reason.unwrap_or(ClientEventSendFailure::SendFailed);
                let mut state = output.state.lock().await;
                let abandoned_lifecycles = state.queue.lifecycle_events();
                state.failure = Some(failure.message);
                state.queue.fail_deliveries(reason);
                output.closed.store(true, Ordering::Release);
                drop(state);
                for lifecycle in abandoned_lifecycles {
                    lifecycle.record(reason);
                }
                output.wake.notify_waiters();
                return;
            }
        }
    }

    async fn write_event(
        writer: &mut ServerEventWriter,
        event: &ServerEvent,
        write_timeout: Duration,
    ) -> Result<(), WorkerWriteFailure> {
        match tokio::time::timeout(write_timeout, writer.send_event(event)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(WorkerWriteFailure {
                message: error.to_string(),
                reason: ClientEventSendFailure::SendFailed,
            }),
            Err(_) => Err(WorkerWriteFailure {
                message: "muxr render/output worker write timed out".to_owned(),
                reason: ClientEventSendFailure::Timeout,
            }),
        }
    }

    fn compose(composer: &mut RenderComposer, input: &RenderInput) -> rootcause::Result<Option<RenderUpdate>> {
        let layout = PaneRenderLayout {
            active_pane: input.active_pane,
            pane_layout: &input.pane_layout,
        };
        if input.force_baseline {
            return composer
                .render_baseline_with_snapshot(
                    input.pane_render,
                    layout,
                    &input.size,
                    &input.attention_panes,
                    |pane_id| input.pane_render_snapshot(pane_id),
                )
                .map(Some);
        }
        composer.render_diff_with_snapshot(
            input.pane_render,
            layout,
            &input.size,
            &input.attention_panes,
            &input.damage,
            |pane_id| input.pane_render_snapshot(pane_id),
        )
    }

    pub async fn shutdown(&mut self) -> rootcause::Result<()> {
        let RenderWorkerMode::Attached { sender, task } = &mut self.mode else {
            return Ok(());
        };
        sender.output.closed.store(true, Ordering::Release);
        sender.output.wake.notify_waiters();
        if let Some(task) = task.take() {
            task.await
                .map_err(|error| rootcause::report!("muxr render/output worker panicked").attach(error.to_string()))?;
        }
        Ok(())
    }
}

impl Drop for RenderWorker {
    fn drop(&mut self) {
        let RenderWorkerMode::Attached { sender, .. } = &self.mode else {
            return;
        };
        sender.output.closed.store(true, Ordering::Release);
        sender.output.wake.notify_waiters();
    }
}

impl ServerEventSink for RenderWorkerSender {
    async fn send_event(&mut self, event: &ServerEvent) -> rootcause::Result<()> {
        let mut state = self.output.state.lock().await;
        self.ensure_open(&state)?;
        state.queue.push_mandatory(event.clone())?;
        drop(state);
        self.output.wake.notify_one();
        Ok(())
    }

    async fn send_event_and_render(
        &mut self,
        event: &ServerEvent,
        command: &ServerRenderCommand,
    ) -> rootcause::Result<()> {
        let ServerRenderCommand::Inputs(input) = command else {
            return Err(rootcause::report!(
                "muxr attached render worker received a precomposed render"
            ));
        };
        let mut state = self.output.state.lock().await;
        self.ensure_open(&state)?;
        state
            .queue
            .push_mandatory_and_replace_render(event.clone(), input.clone())?;
        drop(state);
        self.output.wake.notify_one();
        Ok(())
    }

    async fn send_lifecycle_event(&mut self, event: &ServerEvent) -> rootcause::Result<()> {
        let mut state = self.output.state.lock().await;
        self.ensure_open(&state)?;
        state.queue.push_lifecycle(event.clone())?;
        drop(state);
        self.output.wake.notify_one();
        Ok(())
    }

    async fn send_heartbeat(&mut self) -> rootcause::Result<HeartbeatDelivery> {
        let (delivered, delivery) = tokio::sync::oneshot::channel();
        let mut state = self.output.state.lock().await;
        self.ensure_open(&state)?;
        state.queue.push_heartbeat(delivered)?;
        drop(state);
        self.output.wake.notify_one();
        Ok(HeartbeatDelivery::Queued(delivery))
    }

    async fn send_render(&mut self, command: &ServerRenderCommand) -> rootcause::Result<()> {
        let ServerRenderCommand::Inputs(input) = command else {
            return Err(rootcause::report!(
                "muxr attached render worker received a precomposed render"
            ));
        };
        let mut state = self.output.state.lock().await;
        self.ensure_open(&state)?;
        let _queued = state.queue.replace_render(input.clone());
        drop(state);
        self.output.wake.notify_one();
        Ok(())
    }
}

impl RenderWorkerSender {
    fn ensure_open(&self, state: &OutputState) -> rootcause::Result<()> {
        if let Some(error) = &state.failure {
            return Err(rootcause::report!("muxr render/output worker failed").attach(error.clone()));
        }
        if self.output.closed.load(Ordering::Acquire) {
            return Err(rootcause::report!("muxr render/output worker is closed"));
        }
        Ok(())
    }
}

#[derive(Debug)]
enum ServerOutputCommand {
    Mandatory(MandatoryOutput),
    Render(RenderInput),
}

#[derive(Debug)]
struct MandatoryOutput {
    event: ServerEvent,
    lifecycle: Option<LifecycleEvent>,
    delivered: Option<tokio::sync::oneshot::Sender<Result<tokio::time::Instant, ClientEventSendFailure>>>,
}

#[derive(Clone, Copy, Debug)]
enum LifecycleEvent {
    Deleted,
    Detached,
}

impl LifecycleEvent {
    fn record(self, reason: ClientEventSendFailure) {
        match self {
            Self::Deleted => crate::session::delete::record_delete_ack_send_failure(Some(reason)),
            Self::Detached => crate::client::lifecycle::record_detach_ack_send_failure(Some(reason)),
        }
    }
}

struct WorkerWriteFailure {
    message: String,
    reason: ClientEventSendFailure,
}

struct WorkerCommandFailure {
    message: String,
    reason: Option<ClientEventSendFailure>,
}

impl WorkerCommandFailure {
    fn from_write(failure: WorkerWriteFailure) -> Self {
        Self {
            message: failure.message,
            reason: Some(failure.reason),
        }
    }

    fn from_local(error: &rootcause::Report) -> Self {
        Self {
            message: error.to_string(),
            reason: None,
        }
    }
}

/// Bounded ingress queue for the single server render/output owner.
///
/// Mandatory events retain FIFO order and are always emitted before the latest pending render. Replacements carry a
/// full-damage input, so the worker compares the complete latest state against its last sent frame.
#[derive(Debug)]
pub struct ServerOutputQueue {
    mandatory: VecDeque<MandatoryOutput>,
    pending_render: Option<RenderInput>,
    mandatory_limit: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueRender {
    Queued,
    Replaced,
}

impl ServerOutputQueue {
    pub const fn new(mandatory_limit: usize) -> Self {
        Self {
            mandatory: VecDeque::new(),
            pending_render: None,
            mandatory_limit,
        }
    }

    pub fn push_mandatory(&mut self, event: ServerEvent) -> rootcause::Result<()> {
        self.push_mandatory_output(MandatoryOutput {
            event,
            lifecycle: None,
            delivered: None,
        })
    }

    fn push_lifecycle(&mut self, event: ServerEvent) -> rootcause::Result<()> {
        let lifecycle = match event {
            ServerEvent::Deleted => LifecycleEvent::Deleted,
            ServerEvent::Detached => LifecycleEvent::Detached,
            ServerEvent::Attached(_)
            | ServerEvent::Ping
            | ServerEvent::Pong
            | ServerEvent::Layout(_)
            | ServerEvent::SidebarLayout(_)
            | ServerEvent::PaneRegions(_)
            | ServerEvent::Render(_)
            | ServerEvent::ScrollPaneLineResult { .. }
            | ServerEvent::Error(_) => {
                return Err(rootcause::report!(
                    "muxr output lifecycle command contains a non-lifecycle event"
                ));
            }
        };
        self.push_mandatory_output(MandatoryOutput {
            event,
            lifecycle: Some(lifecycle),
            delivered: None,
        })
    }

    fn push_heartbeat(
        &mut self,
        delivered: tokio::sync::oneshot::Sender<Result<tokio::time::Instant, ClientEventSendFailure>>,
    ) -> rootcause::Result<()> {
        self.push_mandatory_output(MandatoryOutput {
            event: ServerEvent::Ping,
            lifecycle: None,
            delivered: Some(delivered),
        })
    }

    fn push_mandatory_output(&mut self, command: MandatoryOutput) -> rootcause::Result<()> {
        if self.mandatory.len() == self.mandatory_limit {
            return Err(rootcause::report!("muxr server mandatory output queue is full"));
        }
        self.mandatory.push_back(command);
        Ok(())
    }

    fn lifecycle_events(&self) -> Vec<LifecycleEvent> {
        self.mandatory.iter().filter_map(|command| command.lifecycle).collect()
    }

    fn fail_deliveries(&mut self, reason: ClientEventSendFailure) {
        for command in &mut self.mandatory {
            if let Some(delivered) = command.delivered.take() {
                let _sent = delivered.send(Err(reason));
            }
        }
    }

    pub fn push_mandatory_and_replace_render(
        &mut self,
        event: ServerEvent,
        input: RenderInput,
    ) -> rootcause::Result<QueueRender> {
        if self.mandatory.len() == self.mandatory_limit {
            return Err(rootcause::report!("muxr server mandatory output queue is full"));
        }
        self.mandatory.push_back(MandatoryOutput {
            event,
            lifecycle: None,
            delivered: None,
        });
        Ok(self.replace_render(input))
    }

    pub fn replace_render(&mut self, input: RenderInput) -> QueueRender {
        let replaced = self.pending_render.is_some();
        let input = if replaced { input.with_complete_damage() } else { input };
        self.pending_render = Some(input);
        if replaced {
            QueueRender::Replaced
        } else {
            QueueRender::Queued
        }
    }

    fn pop(&mut self) -> Option<ServerOutputCommand> {
        if self
            .mandatory
            .front()
            .is_some_and(|command| matches!(&command.event, ServerEvent::Deleted | ServerEvent::Detached))
        {
            return None;
        }
        self.mandatory
            .pop_front()
            .map(ServerOutputCommand::Mandatory)
            .or_else(|| self.pending_render.take().map(ServerOutputCommand::Render))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use muxr_config::MuxrConfig;
    use muxr_core::PaneId;
    use muxr_core::SessionName;
    use muxr_core::TerminalSize;
    use test_that::prelude::*;

    use super::*;

    fn input(generation: u64) -> rootcause::Result<RenderInput> {
        let config = MuxrConfig::default();
        Ok(RenderInput::new(
            generation,
            PaneRenderConfig {
                mode: crate::pane::borders::BorderRenderMode::Focus,
                border_styles: config.pane_borders,
                pane_attention: config.pane_attention,
                pane_dim: config.pane_dim,
            },
            PaneId::new(1)?,
            PaneLayout::default(),
            BTreeMap::new(),
            TerminalSize::new(1, 1)?,
            Vec::new(),
            ClientRenderDmg::Full,
            false,
        ))
    }

    #[test]
    fn test_server_output_queue_marks_replaced_render_with_complete_damage() -> rootcause::Result<()> {
        let mut queue = ServerOutputQueue::new(1);
        assert_that!(queue.replace_render(input(1)?), eq(QueueRender::Queued));
        assert_that!(queue.replace_render(input(2)?), eq(QueueRender::Replaced));

        let Some(ServerOutputCommand::Render(input)) = queue.pop() else {
            return Err(rootcause::report!("expected pending render"));
        };
        assert_that!(input.generation, eq(2));
        assert_that!(input.damage, eq(ClientRenderDmg::Full));
        Ok(())
    }

    #[test]
    fn test_server_output_queue_preserves_mandatory_before_dependent_render() -> rootcause::Result<()> {
        let mut queue = ServerOutputQueue::new(2);
        queue.push_mandatory(ServerEvent::Ping)?;
        assert_that!(queue.replace_render(input(1)?), eq(QueueRender::Queued));

        if !matches!(
            queue.pop(),
            Some(ServerOutputCommand::Mandatory(MandatoryOutput {
                event: ServerEvent::Ping,
                ..
            }))
        ) {
            return Err(rootcause::report!("expected mandatory event before render"));
        }
        if !matches!(queue.pop(), Some(ServerOutputCommand::Render(_))) {
            return Err(rootcause::report!(
                "expected non-superseded render after mandatory event"
            ));
        }
        Ok(())
    }

    #[test]
    fn test_server_output_queue_atomically_replaces_old_render_with_complete_successor() -> rootcause::Result<()> {
        let mut queue = ServerOutputQueue::new(2);
        assert_that!(queue.replace_render(input(1)?), eq(QueueRender::Queued));
        assert_that!(
            queue.push_mandatory_and_replace_render(ServerEvent::Ping, input(2)?)?,
            eq(QueueRender::Replaced)
        );

        if !matches!(
            queue.pop(),
            Some(ServerOutputCommand::Mandatory(MandatoryOutput {
                event: ServerEvent::Ping,
                ..
            }))
        ) {
            return Err(rootcause::report!("expected atomic mandatory event before successor"));
        }
        let Some(ServerOutputCommand::Render(input)) = queue.pop() else {
            return Err(rootcause::report!("expected atomic successor render"));
        };
        assert_that!(input.generation, eq(2));
        assert_that!(input.damage, eq(ClientRenderDmg::Full));
        assert_that!(queue.pop(), none());
        Ok(())
    }

    #[test]
    fn test_lifecycle_command_is_held_for_shutdown_and_keeps_failure_tracing_kind() -> rootcause::Result<()> {
        let mut queue = ServerOutputQueue::new(1);
        queue.push_lifecycle(ServerEvent::Detached)?;
        assert_that!(queue.pop(), none());
        let Some(MandatoryOutput {
            event: ServerEvent::Detached,
            lifecycle: Some(LifecycleEvent::Detached),
            ..
        }) = queue.mandatory.pop_front()
        else {
            return Err(rootcause::report!("expected lifecycle command"));
        };
        Ok(())
    }

    #[test]
    fn test_earlier_worker_failure_records_queued_detach_delivery_failure() -> rootcause::Result<()> {
        let mut queue = ServerOutputQueue::new(2);
        queue.push_mandatory(ServerEvent::Ping)?;
        queue.push_lifecycle(ServerEvent::Detached)?;
        if !matches!(
            queue.pop(),
            Some(ServerOutputCommand::Mandatory(MandatoryOutput {
                event: ServerEvent::Ping,
                ..
            }))
        ) {
            return Err(rootcause::report!("expected earlier mandatory command"));
        }
        let abandoned_lifecycles = queue.lifecycle_events();
        assert_that!(abandoned_lifecycles.len(), eq(1));

        let session = SessionName::default();
        let log = crate::session::tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            for lifecycle in abandoned_lifecycles {
                lifecycle.record(ClientEventSendFailure::Timeout);
            }
            Ok(())
        })?;

        assert_that!(log, contains_substring("kind=\"detach_ack_send_failed\""));
        assert_that!(log, contains_substring("reason=\"timeout\""));
        Ok(())
    }

    #[test]
    fn test_superseded_render_does_not_generate_a_baseline_inside_worker_composer() -> rootcause::Result<()> {
        let mut composer = RenderComposer::default();
        let first_input = input(1)?;
        let first = RenderWorker::compose(&mut composer, &first_input)?
            .ok_or_else(|| rootcause::report!("initial worker render was empty"))?;
        if !matches!(first, RenderUpdate::Baseline(_)) {
            return Err(rootcause::report!("initial worker render was not a baseline"));
        }

        let successor_input = input(2)?;
        let successor = RenderWorker::compose(&mut composer, &successor_input)?;
        assert_that!(matches!(successor, Some(RenderUpdate::Baseline(_))), eq(false));
        Ok(())
    }
}
