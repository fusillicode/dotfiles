use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use kanal::Receiver;
use kanal::Sender;
use muxr_core::AttachRequest;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::ServerEvent;
use muxr_core::TerminalSize;
use muxr_transport::ServerConnection;
use muxr_transport::ServerRequestReader;
use rootcause::report;
use tokio::sync::mpsc::error::TryRecvError;

use super::quiet::QuietTurn;
use crate::client::heartbeat::HeartbeatStatus;
use crate::client::heartbeat::HeartbeatTracker;
use crate::client::timers::ClientTimers;
use crate::client::timers::QuietDeadline;
use crate::keyboard_input::ServerInputMode;
use crate::pane::cmd::NvimState;
use crate::pane::cmd::PaneCmdObservation;
use crate::pane::cmd::PaneCmdSnapshot;
use crate::pane::fullscreen::PaneFullscreen;
use crate::pane::runtime::PaneRuntimes;
use crate::pane::tracked_process::PaneTrackedProcesses;
use crate::pty::PtyEvent;
use crate::pty::PtySinkGuard;
use crate::render_state::ClientLifecycleAction;
use crate::render_state::ClientRenderDmg;
use crate::render_state::ClientSessionFlow;
use crate::render_state::ClientSessionSelectBias;
use crate::render_worker::RenderWorker;
use crate::scrollback_editor::ScrollbackEditorState;
use crate::server::ServerConfig;
use crate::session::delete::DeleteSessions;
use crate::session::runtime::PANE_OUTPUT_EVENT_CHANNEL_LIMIT;
use crate::session::runtime::ReapResult;
use crate::session::runtime::SessionClientMessage;
use crate::session::runtime::SessionPaneOutputMessage;
use crate::session::runtime::SessionRuntimeTimerMessage;
use crate::state::PaneTreeRightPane;
use crate::state::SessionLayout;

#[cfg(test)]
mod tests;

// A quiet-boundary batch coalesces many PTY wakeup markers into one handler call, but stays small enough to yield back
// to the request/output select loop before quiet clearing if the channel is full.
const QUIET_OUTPUT_DRAIN_BATCH_LIMIT: usize = 32;

struct ClientPtySink {
    guard: PtySinkGuard,
    pane_id: PaneId,
    output_wakeup_pending: Arc<AtomicBool>,
}

pub struct ClientSessionState<'a> {
    pub pane_tracked_processes: PaneTrackedProcesses,
    pub config: &'a ServerConfig,
    pub delete_sessions: &'a DeleteSessions,
    pub input_mode: ServerInputMode,
    pub last_layout_snapshot: LayoutSnapshot,
    pub layout: &'a mut SessionLayout,
    pub pane_fullscreen: PaneFullscreen,
    pty_event_sender: &'a Sender<PtyEvent>,
    pub render_worker: &'a mut RenderWorker,
    pub runtimes: &'a mut PaneRuntimes,
    pub scrollback_editor: Option<ScrollbackEditorState>,
    sink_guards: &'a mut Vec<ClientPtySink>,
    pub terminal_size: TerminalSize,
}

impl ClientSessionState<'_> {
    pub(crate) fn open_file_pane_route(&self, source_pane_id: PaneId) -> rootcause::Result<OpenFilePaneRoute> {
        let right_pane = self.layout.active_tab()?.pane_tree.right_pane_of(source_pane_id);
        let nvim_state = match right_pane {
            PaneTreeRightPane::Pane(right_pane_id) => self.pane_nvim_state(right_pane_id),
            PaneTreeRightPane::Missing => NvimState::Unknown,
        };
        Ok(self::open_file_pane_route_for_right_pane(right_pane, nvim_state))
    }

    fn pane_nvim_state(&self, pane_id: PaneId) -> NvimState {
        let Ok(handle) = self.runtimes.handle(pane_id) else {
            return NvimState::Unknown;
        };
        let Ok(snapshot) = PaneCmdSnapshot::try_from(&handle) else {
            return NvimState::Unknown;
        };
        PaneCmdObservation::from(&snapshot).nvim_state()
    }

    pub(crate) fn focus_pane_for_open_file(
        &mut self,
        pane_id: PaneId,
        timers: &mut ClientTimers,
    ) -> rootcause::Result<()> {
        let previous_pane = self.layout.active_pane_id()?;
        self.layout.active_tab_mut()?.focus_pane(pane_id)?;
        if previous_pane != pane_id {
            crate::pane::focus::write_active_pane_focus_events(previous_pane, self)?;
            crate::state::persisted::write_metadata(&self.config.paths, self.layout)?;
        }
        let _changes = self.pane_tracked_processes.acknowledge_active_pane_attention(
            self.config.user_config.as_ref(),
            self.layout,
            self.runtimes,
            std::time::Instant::now(),
        )?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&self.pane_tracked_processes, self.layout)?;
        Ok(())
    }
}

fn open_file_pane_route_for_right_pane(right_pane: PaneTreeRightPane, nvim_state: NvimState) -> OpenFilePaneRoute {
    match right_pane {
        PaneTreeRightPane::Pane(right_pane_id) if nvim_state == NvimState::Running => {
            OpenFilePaneRoute::ExistingNvim(right_pane_id)
        }
        PaneTreeRightPane::Pane(_) | PaneTreeRightPane::Missing => OpenFilePaneRoute::NewRightSplit,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenFilePaneRoute {
    ExistingNvim(PaneId),
    NewRightSplit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReapedPanes {
    Unchanged,
    LayoutChanged,
    Stop,
}

fn attach_pane_sinks(runtimes: &PaneRuntimes, sender: &Sender<PtyEvent>) -> rootcause::Result<Vec<ClientPtySink>> {
    let output_wakeup_pending = Arc::new(AtomicBool::new(false));
    runtimes
        .pane_ids()
        .into_iter()
        .map(|pane_id| {
            Ok(ClientPtySink {
                guard: runtimes
                    .handle(pane_id)?
                    .attach_sink_with_output_wakeup(sender.clone(), Arc::clone(&output_wakeup_pending)),
                pane_id,
                output_wakeup_pending: Arc::clone(&output_wakeup_pending),
            })
        })
        .collect()
}

fn attach_pane_sink(
    runtimes: &PaneRuntimes,
    sender: &Sender<PtyEvent>,
    pane_id: PaneId,
    output_wakeup_pending: &Arc<AtomicBool>,
) -> rootcause::Result<ClientPtySink> {
    Ok(ClientPtySink {
        guard: runtimes
            .handle(pane_id)?
            .attach_sink_with_output_wakeup(sender.clone(), Arc::clone(output_wakeup_pending)),
        pane_id,
        output_wakeup_pending: Arc::clone(output_wakeup_pending),
    })
}

pub fn attach_pane_sink_to_state(state: &mut ClientSessionState<'_>, pane_id: PaneId) -> rootcause::Result<()> {
    let output_wakeup_pending = state.sink_guards.first().map_or_else(
        || Arc::new(AtomicBool::new(false)),
        |sink| Arc::clone(&sink.output_wakeup_pending),
    );
    state.sink_guards.push(self::attach_pane_sink(
        state.runtimes,
        state.pty_event_sender,
        pane_id,
        &output_wakeup_pending,
    )?);
    Ok(())
}

fn remove_pane_client_resources(state: &mut ClientSessionState<'_>, pane_id: PaneId) {
    // This cleanup is used during attach/session teardown paths without live client timers.
    state.sink_guards.retain(|sink| sink.pane_id != pane_id);
    // Pane IDs are allocated from the live layout max, so a removed high ID can be reused before the next quiet sweep.
    state.pane_tracked_processes.remove_pane(pane_id);
}

fn remove_live_pane_tracking(
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    pane_id: PaneId,
) -> rootcause::Result<()> {
    // Prompt-submit sampling fires after a short delay, so live pane removal must clear the timer entry before the
    // runtime disappears; otherwise a later sample can ask for a stale pane handle and tear down the client session.
    state.pane_tracked_processes.remove_pane(pane_id);
    timers.remove_cmd_handoff_sample_pane(pane_id)?;
    timers.remove_output_activity_sample_pane(pane_id)
}

pub fn remove_pane_from_client_state(
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    pane_id: PaneId,
) -> rootcause::Result<()> {
    state.sink_guards.retain(|sink| sink.pane_id != pane_id);
    self::remove_live_pane_tracking(state, timers, pane_id)
}

pub async fn handle_client(
    config: &ServerConfig,
    connection: ServerConnection,
    attach_request: AttachRequest,
    delete_sessions: &DeleteSessions,
    layout: &mut SessionLayout,
    runtimes: &mut PaneRuntimes,
) -> rootcause::Result<ClientSessionCompletion> {
    crate::screen_render::resize_panes_to_layout(layout, runtimes, &attach_request.terminal_size)?;
    let (pty_event_sender, pty_event_receiver) = kanal::bounded(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
    let mut sink_guards = self::attach_pane_sinks(runtimes, &pty_event_sender)?;
    let (mut request_reader, event_writer) = connection.split();
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    pane_tracked_processes.observe_all_runtime_pane_cmds(
        config.user_config.as_ref(),
        layout,
        runtimes,
        Instant::now(),
    )?;
    let mut render_worker = RenderWorker::default();
    let mut event_writer = render_worker.attach_writer(event_writer, config.client_write_timeout)?;
    let (layout_snapshot, pane_regions, initial_render) = crate::screen_render::initial_client_render_input(
        config,
        layout,
        runtimes,
        &pane_tracked_processes,
        &attach_request.terminal_size,
        &mut render_worker,
    )?;
    let last_layout_snapshot = layout_snapshot.clone();
    if crate::event_writer::send_event_with_timeout(
        &mut event_writer,
        &ServerEvent::Attached(muxr_core::AttachAccepted {
            layout: layout_snapshot,
            pane_regions,
        }),
        config.client_write_timeout,
    )
    .await?
    .session_flow()
        == ClientSessionFlow::Disconnect
    {
        render_worker.shutdown().await?;
        return Ok(ClientSessionCompletion {
            render_worker,
            result: Ok(()),
        });
    }
    let initial_render = render_worker.stage_render(initial_render)?;
    if crate::event_writer::send_render_with_timeout(&mut event_writer, &initial_render, config.client_write_timeout)
        .await?
        .session_flow()
        == ClientSessionFlow::Disconnect
    {
        render_worker.shutdown().await?;
        return Ok(ClientSessionCompletion {
            render_worker,
            result: Ok(()),
        });
    }

    let (mut async_pty_receiver, bridge_handle) = self::spawn_pty_event_bridge(pty_event_receiver);
    let mut client_state = ClientSessionState {
        pane_tracked_processes,
        config,
        delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot,
        layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size: attach_request.terminal_size,
    };
    let result = self::run_client_session(
        &mut request_reader,
        &mut event_writer,
        &mut client_state,
        &mut async_pty_receiver,
    )
    .await;
    let restore_result = crate::scrollback_editor::restore_without_render(&mut client_state);
    if let Ok(outcome) = &restore_result
        && let Some(editor_pane_id) = outcome.editor_pane_id
    {
        self::remove_pane_client_resources(&mut client_state, editor_pane_id);
    }
    drop(client_state);

    drop(sink_guards);
    drop(pty_event_sender);
    drop(async_pty_receiver);
    bridge_handle
        .await
        .map_err(|error| report!("muxr server pty bridge task panicked").attach(format!("{error}")))?;
    let result = match result {
        Ok(()) => restore_result.map(|_| ()),
        Err(error) => {
            let _ = restore_result.inspect_err(|restore_error| {
                crate::session::tracing::scrollback::restore_failed(restore_error);
            });
            Err(error)
        }
    };
    Ok(ClientSessionCompletion { render_worker, result })
}

pub struct ClientSessionCompletion {
    render_worker: RenderWorker,
    result: rootcause::Result<()>,
}

impl ClientSessionCompletion {
    pub async fn finish(mut self) -> rootcause::Result<()> {
        self.render_worker.shutdown().await?;
        self.result
    }
}

fn spawn_pty_event_bridge(
    pty_event_receiver: Receiver<PtyEvent>,
) -> (
    tokio::sync::mpsc::Receiver<SessionPaneOutputMessage>,
    tokio::task::JoinHandle<()>,
) {
    let (async_pty_sender, async_pty_receiver) = tokio::sync::mpsc::channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
    // kanal 0.1.1 documents async receives as unsuitable for `tokio::select!` cancellation. Keep this bridge so the
    // client loop can select PTY output against requests and timers without risking a lost output wakeup.
    let bridge_handle =
        tokio::task::spawn_blocking(move || self::forward_pty_events_to_async(&pty_event_receiver, &async_pty_sender));
    (async_pty_receiver, bridge_handle)
}

fn forward_pty_events_to_async(
    pty_event_receiver: &Receiver<PtyEvent>,
    async_pty_sender: &tokio::sync::mpsc::Sender<SessionPaneOutputMessage>,
) {
    while let Ok(event) = pty_event_receiver.recv() {
        if async_pty_sender
            .blocking_send(SessionPaneOutputMessage::from(event))
            .is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
fn pty_event_channel() -> (Sender<PtyEvent>, Receiver<PtyEvent>) {
    kanal::bounded(PANE_OUTPUT_EVENT_CHANNEL_LIMIT)
}

async fn run_client_session(
    request_reader: &mut ServerRequestReader,
    event_writer: &mut impl crate::event_writer::ServerEventSink,
    state: &mut ClientSessionState<'_>,
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<SessionPaneOutputMessage>,
) -> rootcause::Result<()> {
    self::run_client_session_loop(
        request_reader,
        event_writer,
        state,
        pty_event_receiver,
        ClientSessionSelectBias::Output,
    )
    .await
}

#[cfg(test)]
async fn run_test_client_session(
    request_reader: &mut ServerRequestReader,
    event_writer: &mut impl crate::event_writer::ServerEventSink,
    state: &mut ClientSessionState<'_>,
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<SessionPaneOutputMessage>,
    select_bias: ClientSessionSelectBias,
) -> rootcause::Result<()> {
    let result =
        self::run_client_session_loop(request_reader, event_writer, state, pty_event_receiver, select_bias).await;
    state.render_worker.shutdown().await?;
    result
}

#[expect(
    clippy::too_many_lines,
    reason = "the two biased select branches keep request/output priority ordering explicit"
)]
async fn run_client_session_loop(
    request_reader: &mut ServerRequestReader,
    event_writer: &mut impl crate::event_writer::ServerEventSink,
    state: &mut ClientSessionState<'_>,
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<SessionPaneOutputMessage>,
    mut select_bias: ClientSessionSelectBias,
) -> rootcause::Result<()> {
    let mut timers = ClientTimers::new(state.config)?;
    timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
    let mut heartbeat = HeartbeatTracker::default();
    let mut render_dmg = ClientRenderDmg::Clean;
    let mut quiet_turn = QuietTurn::default();

    loop {
        if heartbeat.sync_delivery() == ClientSessionFlow::Disconnect {
            return Ok(());
        }
        if crate::client::lifecycle::client_should_exit(
            state.sink_guards.iter().map(|sink| sink.guard.output_freshness()),
            state.config.client_heartbeat_timeout,
            state.delete_sessions,
            heartbeat.response_started_at(),
        ) == ClientLifecycleAction::Exit
        {
            return Ok(());
        }
        timers.sync_render_deadline(&render_dmg)?;
        let ready_quiet = quiet_turn.take_ready(timers.tracked_process_quiet_deadline());
        let mut skip_quiet_this_turn = false;
        if ready_quiet == QuietTurn::DrainBeforeClear {
            match self::drain_queued_output_before_quiet(
                pty_event_receiver,
                event_writer,
                state,
                &mut timers,
                &mut render_dmg,
            )
            .await?
            {
                OutputDrain::NoOutput => {}
                OutputDrain::Output => select_bias = ClientSessionSelectBias::Request,
                OutputDrain::BatchLimitReached => {
                    select_bias = ClientSessionSelectBias::Request;
                    // A full drain batch means output stayed hot. Re-arm quiet so requests get a turn before Busy can
                    // clear.
                    quiet_turn.defer_if_elapsed(timers.tracked_process_quiet_deadline());
                    skip_quiet_this_turn = true;
                }
                OutputDrain::Detached => return Ok(()),
            }
        }
        if !skip_quiet_this_turn
            && ready_quiet == QuietTurn::DrainBeforeClear
            && timers.tracked_process_quiet_deadline() == crate::client::timers::QuietDeadline::Elapsed
        {
            if self::handle_session_runtime_timer_message(
                SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached,
                event_writer,
                state,
                &mut timers,
                &mut heartbeat,
                &mut render_dmg,
            )
            .await?
                == ClientSessionFlow::Disconnect
            {
                return Ok(());
            }
            continue;
        }

        if select_bias == ClientSessionSelectBias::Request {
            tokio::select! {
                biased;
                _ = timers.heartbeat.tick() => {
                    if self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::HeartbeatTick,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat,
                        &mut render_dmg,
                    ).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                },
                () = timers.render_sleep.as_mut() => {
                    if self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::RenderDeadlineReached,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat,
                        &mut render_dmg,
                    ).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                },
                () = timers.cmd_handoff_sample.as_mut() => {
                    if self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::CmdHandoffSampleReady,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat,
                        &mut render_dmg,
                    ).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                },
                () = timers.output_activity_sample.as_mut() => {
                    if crate::screen_render::handle_output_activity_sample(&mut timers, event_writer, state, &mut render_dmg).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                },
                request = request_reader.recv_request() => {
                    let message = SessionClientMessage::from_request(request?);
                    if crate::request_router::handle_client_message(message, event_writer, state, &mut timers, &mut heartbeat, &mut render_dmg).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                    select_bias = ClientSessionSelectBias::Output;
                    quiet_turn.defer_if_elapsed(timers.tracked_process_quiet_deadline());
                },
                event = pty_event_receiver.recv() => {
                    select_bias = ClientSessionSelectBias::Request;
                    if crate::pty_output::handle_pane_output_message(event, event_writer, state, &mut timers, &mut render_dmg).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                    quiet_turn.defer_if_elapsed(timers.tracked_process_quiet_deadline());
                },
                () = timers.tracked_process_quiet_sleep.as_mut() => {
                    if self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat,
                        &mut render_dmg,
                    ).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                },
            }
        } else {
            tokio::select! {
                biased;
                _ = timers.heartbeat.tick() => {
                    if self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::HeartbeatTick,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat,
                        &mut render_dmg,
                    ).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                },
                () = timers.render_sleep.as_mut() => {
                    if self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::RenderDeadlineReached,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat,
                        &mut render_dmg,
                    ).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                },
                () = timers.cmd_handoff_sample.as_mut() => {
                    if self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::CmdHandoffSampleReady,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat,
                        &mut render_dmg,
                    ).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                },
                () = timers.output_activity_sample.as_mut() => {
                    if crate::screen_render::handle_output_activity_sample(&mut timers, event_writer, state, &mut render_dmg).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                },
                event = pty_event_receiver.recv() => {
                    // Output gets one turn, then client requests get first chance so detach/pong cannot starve.
                    select_bias = ClientSessionSelectBias::Request;
                    if crate::pty_output::handle_pane_output_message(event, event_writer, state, &mut timers, &mut render_dmg).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                    quiet_turn.defer_if_elapsed(timers.tracked_process_quiet_deadline());
                },
                request = request_reader.recv_request() => {
                    let message = SessionClientMessage::from_request(request?);
                    if crate::request_router::handle_client_message(message, event_writer, state, &mut timers, &mut heartbeat, &mut render_dmg).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                    select_bias = ClientSessionSelectBias::Output;
                    quiet_turn.defer_if_elapsed(timers.tracked_process_quiet_deadline());
                },
                () = timers.tracked_process_quiet_sleep.as_mut() => {
                    if self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat,
                        &mut render_dmg,
                    ).await? == ClientSessionFlow::Disconnect {
                        return Ok(());
                    }
                },
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputDrain {
    NoOutput,
    Output,
    BatchLimitReached,
    Detached,
}

async fn drain_queued_output_before_quiet(
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<SessionPaneOutputMessage>,
    event_writer: &mut impl crate::event_writer::ServerEventSink,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    render_dmg: &mut ClientRenderDmg,
) -> rootcause::Result<OutputDrain> {
    let mut pane_exited = false;
    let mut pane_output_ready = false;
    let mut batch_limit_reached = false;

    for remaining_events in (1..=QUIET_OUTPUT_DRAIN_BATCH_LIMIT).rev() {
        match pty_event_receiver.try_recv() {
            Ok(SessionPaneOutputMessage::PaneExited) => pane_exited = true,
            Ok(SessionPaneOutputMessage::PaneOutputReady) => pane_output_ready = true,
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                if !pane_exited && !pane_output_ready {
                    if crate::pty_output::handle_pane_output_message(None, event_writer, state, timers, render_dmg)
                        .await?
                        == ClientSessionFlow::Continue
                    {
                        return Ok(OutputDrain::NoOutput);
                    }
                    return Ok(OutputDrain::Detached);
                }
                break;
            }
        }
        batch_limit_reached = remaining_events == 1;
    }

    let event = if pane_output_ready {
        // PTY wakeups are sticky hints. One output-ready pass drains dirty panes, title changes, and exits.
        Some(SessionPaneOutputMessage::PaneOutputReady)
    } else if pane_exited {
        Some(SessionPaneOutputMessage::PaneExited)
    } else {
        return Ok(OutputDrain::NoOutput);
    };

    if crate::pty_output::handle_pane_output_message(event, event_writer, state, timers, render_dmg).await?
        == ClientSessionFlow::Disconnect
    {
        return Ok(OutputDrain::Detached);
    }
    if batch_limit_reached && timers.tracked_process_quiet_deadline() == QuietDeadline::Elapsed {
        return Ok(OutputDrain::BatchLimitReached);
    }
    Ok(OutputDrain::Output)
}

async fn handle_session_runtime_timer_message(
    message: SessionRuntimeTimerMessage,
    event_writer: &mut impl crate::event_writer::ServerEventSink,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    heartbeat: &mut impl HeartbeatStatus,
    render_dmg: &mut ClientRenderDmg,
) -> rootcause::Result<ClientSessionFlow> {
    match message {
        SessionRuntimeTimerMessage::HeartbeatTick => {
            crate::client::heartbeat::send_if_idle(event_writer, state.config.client_write_timeout, heartbeat).await
        }
        SessionRuntimeTimerMessage::RenderDeadlineReached => {
            let flow = crate::screen_render::flush_render_diff(event_writer, state, render_dmg).await?;
            // `Sleep` stays ready after it fires. Complete the frame immediately so the one-shot wakeup is disabled
            // and the next dirty frame is rate-limited from this render attempt.
            timers.complete_render_frame()?;
            Ok(flow)
        }
        SessionRuntimeTimerMessage::CmdHandoffSampleReady => {
            crate::screen_render::handle_cmd_handoff_sample(timers, event_writer, state, render_dmg).await
        }
        SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached => {
            timers.disable_tracked_process_quiet_sleep()?;
            if crate::screen_render::flush_pane_attention(timers, event_writer, state, render_dmg).await?
                == ClientSessionFlow::Disconnect
            {
                return Ok(ClientSessionFlow::Disconnect);
            }
            timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
            Ok(ClientSessionFlow::Continue)
        }
    }
}

pub async fn handle_reaped_panes(
    state: &mut ClientSessionState<'_>,
    event_writer: &mut impl crate::event_writer::ServerEventSink,
    timers: &mut ClientTimers,
) -> rootcause::Result<ReapedPanes> {
    let previous_pane_before_restore = state.layout.active_pane_id()?;
    let restored_editor = crate::scrollback_editor::restore_before_reap_if_needed(state)?;
    if let Some(editor_pane_id) = restored_editor.editor_pane_id {
        self::remove_pane_from_client_state(state, timers, editor_pane_id)?;
    }
    let previous_pane_before_reap = state.layout.active_pane_id()?;
    match crate::session::runtime::reap_exited_panes(state.config, state.layout, state.runtimes)? {
        ReapResult::Final => Ok(ReapedPanes::Stop),
        ReapResult::NoExitedPanes => {
            if restored_editor.status() == crate::scrollback_editor::ScrollbackEditorRestoreStatus::Unchanged {
                return Ok(ReapedPanes::Unchanged);
            }
            crate::pane::focus::write_active_pane_focus_events(previous_pane_before_restore, state)?;
            self::acknowledge_active_tracked_process(state)?;
            match crate::screen_render::send_layout_and_baseline(event_writer, state).await? {
                ClientSessionFlow::Continue => Ok(ReapedPanes::LayoutChanged),
                ClientSessionFlow::Disconnect => Ok(ReapedPanes::Stop),
            }
        }
        ReapResult::Removed { pane_ids } => {
            for pane_id in &pane_ids {
                self::remove_live_pane_tracking(state, timers, *pane_id)?;
            }
            // Keep the common single-pane reap allocation-free. Batched reaps build membership once so cleanup does
            // not become sink_guards * removed_panes work.
            match pane_ids.as_slice() {
                [] => {}
                [pane_id] => state.sink_guards.retain(|sink| sink.pane_id != *pane_id),
                pane_ids => {
                    let pane_ids: BTreeSet<_> = pane_ids.iter().copied().collect();
                    state.sink_guards.retain(|sink| !pane_ids.contains(&sink.pane_id));
                }
            }
            let previous_pane =
                if restored_editor.status() == crate::scrollback_editor::ScrollbackEditorRestoreStatus::Restored {
                    previous_pane_before_restore
                } else {
                    previous_pane_before_reap
                };
            crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
            self::acknowledge_active_tracked_process(state)?;
            match crate::screen_render::resize_panes_and_render(event_writer, state).await? {
                ClientSessionFlow::Continue => Ok(ReapedPanes::LayoutChanged),
                ClientSessionFlow::Disconnect => Ok(ReapedPanes::Stop),
            }
        }
    }
}

pub fn acknowledge_active_tracked_process(
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<crate::pane::tracked_process::TrackedProcessChanges> {
    let active_pane = state.layout.active_pane_id()?;
    // Close/reap fallback focus is not a runtime sample. Only acknowledge already-known attention here; command
    // observation stays with output and explicit focus paths so a transient shell sample cannot clear unrelated work.
    Ok(state.pane_tracked_processes.acknowledge_attention(active_pane))
}
