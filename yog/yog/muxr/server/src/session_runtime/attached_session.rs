use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use muxr_config::ScrollbackDumpStyle;
use muxr_core::AttachAccepted;
use muxr_core::AttachRequest;
use muxr_core::ClientKey;
use muxr_core::ClientMouseEvent;
use muxr_core::ClientMousePosition;
use muxr_core::ClientRequest;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::PaneScrollDirection;
use muxr_core::RenderUpdate;
use muxr_core::ServerError;
use muxr_core::ServerEvent;
use muxr_core::SessionPaths;
use muxr_core::TabId;
use muxr_core::TerminalSize;
use muxr_transport::ServerConnection;
use muxr_transport::ServerEventWriter;
use muxr_transport::ServerRequestReader;
use rootcause::report;
use tokio::sync::mpsc::error::TrySendError;

use crate::attached_client_timers::AttachedClientTimers;
use crate::keyboard_input::ClientCmd;
use crate::keyboard_input::KeyResolution;
use crate::keyboard_input::ServerInputMode;
use crate::keyboard_input::TabCmd;
use crate::pane_close::ClosePaneOutcome;
use crate::pane_close::PaneExitOutcome;
use crate::pane_fullscreen::PaneFullscreen;
use crate::pane_layout::PaneLayout;
use crate::pane_layout::PaneRegion;
use crate::pane_render::PaneRenderConfig;
use crate::pane_render::PaneRenderLayout;
use crate::pane_render::RenderComposer;
use crate::pane_render::RenderDiffReason;
use crate::pane_runtime::PaneRuntimeMetadata;
use crate::pane_runtime::PaneRuntimes;
use crate::pane_scroll::PaneScrollAmount;
use crate::pane_tracked_process::PaneTrackedProcessSnapshot;
use crate::pane_tracked_process::PaneTrackedProcesses;
use crate::pane_tracked_process::TrackedProcessAttention;
use crate::pane_tracked_process::TrackedProcessUserInteraction;
use crate::pty::PtyEvent;
use crate::pty::PtyHandle;
use crate::pty::PtySinkGuard;
use crate::scrollback_editor::ScrollbackEditorState;
use crate::server::ServerConfig;
use crate::session_runtime::PANE_OUTPUT_EVENT_CHANNEL_LIMIT;
use crate::session_runtime::SessionAttachedClientMessage;
use crate::session_runtime::SessionAttachedClientTaskMessage;
use crate::session_runtime::SessionPaneOutputMessage;
use crate::session_runtime::SessionRuntime;
use crate::session_runtime::SessionRuntimeState;
use crate::session_runtime::SessionRuntimeTimerMessage;
use crate::session_tracing::ClientEventSendFailure;
use crate::sessions_delete::DeleteSessions;
use crate::state::SessionLayout;
use crate::terminal::TerminalFocusEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReapResult {
    Final,
    NoExitedPanes,
    Removed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScrollbackEditorCmdAction {
    Ignore,
    Restore,
    Run(ClientCmd),
}

struct AttachedPtySink {
    guard: PtySinkGuard,
    pane_id: PaneId,
}

struct AttachedSessionState<'a> {
    pane_tracked_processes: PaneTrackedProcesses,
    config: &'a ServerConfig,
    delete_sessions: &'a DeleteSessions,
    input_mode: ServerInputMode,
    last_layout_snapshot: LayoutSnapshot,
    layout: &'a mut SessionLayout,
    pane_fullscreen: PaneFullscreen,
    pane_regions: PaneRegionsSnapshot,
    pty_event_sender: &'a mpsc::SyncSender<PtyEvent>,
    render_composer: &'a mut RenderComposer,
    runtimes: &'a mut PaneRuntimes,
    scrollback_editor: Option<ScrollbackEditorState>,
    sink_guards: &'a mut Vec<AttachedPtySink>,
    terminal_size: TerminalSize,
}

pub struct AttachedClientTaskRuntime {
    completion_sender: tokio::sync::mpsc::Sender<SessionAttachedClientTaskMessage>,
    delete_sessions: std::sync::Arc<DeleteSessions>,
    state: SessionRuntimeState,
}

impl AttachedClientTaskRuntime {
    #[tracing::instrument(name = "muxr_session", skip_all, fields(session = %config.session))]
    pub async fn run_attached_client(
        mut self,
        config: &ServerConfig,
        connection: ServerConnection,
        attach_request: AttachRequest,
    ) -> rootcause::Result<()> {
        let result = self::handle_client(
            config,
            connection,
            attach_request,
            &self.delete_sessions,
            &mut self.state,
        )
        .await;
        match self
            .completion_sender
            .try_send(SessionAttachedClientTaskMessage::Finished(self.state))
        {
            // Closed means the session loop is already gone; a full channel can strand live state without another
            // recovery path.
            Ok(()) | Err(TrySendError::Closed(_)) => {}
            Err(TrySendError::Full(_)) => {
                crate::session_tracing::attached_client::state_handoff_failed("channel_full");
            }
        }
        result
    }
}

impl SessionRuntime {
    pub fn attached_client_task_runtime(
        &mut self,
        completion_sender: tokio::sync::mpsc::Sender<SessionAttachedClientTaskMessage>,
        delete_sessions: std::sync::Arc<DeleteSessions>,
    ) -> rootcause::Result<AttachedClientTaskRuntime> {
        Ok(AttachedClientTaskRuntime {
            completion_sender,
            delete_sessions,
            state: self.take_state_for_attach()?,
        })
    }

    pub fn reap_exited_panes(&mut self, config: &ServerConfig) -> rootcause::Result<ReapResult> {
        let Some(state) = &mut self.state else {
            return Ok(ReapResult::NoExitedPanes);
        };
        self::reap_exited_panes(config, &mut state.layout, &mut state.pane_runtimes)
    }

    pub fn pane_runtime_set_empty(&self) -> bool {
        self.state.as_ref().is_some_and(|state| state.pane_runtimes.is_empty())
    }
}

pub fn resize_panes_to_layout(
    layout: &SessionLayout,
    runtimes: &PaneRuntimes,
    size: &TerminalSize,
) -> rootcause::Result<()> {
    let regions = layout.pane_regions(size)?;
    runtimes.resize_panes(&regions)
}

pub fn reap_exited_panes(
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &mut PaneRuntimes,
) -> rootcause::Result<ReapResult> {
    let exited_panes = runtimes.exited_panes()?;
    if exited_panes.is_empty() {
        return Ok(ReapResult::NoExitedPanes);
    }

    let exited_at = crate::server::unix_timestamp_millis()?;
    let mut result = ReapResult::Removed;
    let _ = runtimes.sync_layout_terminal_titles(layout)?;
    let mut removed_panes = Vec::new();
    for (pane_id, exit_status) in &exited_panes {
        match layout.remove_exited_pane(*pane_id, exited_at, exit_status.clone())? {
            PaneExitOutcome::Final => result = ReapResult::Final,
            PaneExitOutcome::Removed => {}
        }
        removed_panes.push(pane_id);
    }
    crate::state::persisted::write_metadata(&config.paths, layout)?;
    for pane_id in removed_panes {
        runtimes.remove(*pane_id);
    }

    Ok(result)
}

pub fn initial_attached_render(
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    pane_tracked_processes: &PaneTrackedProcesses,
    terminal_size: &TerminalSize,
) -> rootcause::Result<(LayoutSnapshot, PaneRegionsSnapshot, RenderComposer, RenderUpdate)> {
    let mut render_composer = RenderComposer::default();
    let tracked_processes = pane_tracked_processes.snapshot();
    let layout_snapshot = self::layout_snapshot_and_persist(&config.paths, layout, runtimes, &tracked_processes)?;
    let pane_layout = PaneFullscreen::default().pane_layout(layout, terminal_size)?;
    let pane_regions = self::pane_regions_snapshot(&pane_layout, runtimes)?;
    let attention_panes = self::attention_pane_ids(layout, pane_tracked_processes);
    let render_baseline = render_composer.render_baseline(
        PaneRenderConfig {
            border_styles: config.user_config.pane_borders,
            mode: crate::pane_borders::BorderRenderMode::Focus,
            pane_attention: config.user_config.pane_attention,
            pane_dim: config.user_config.pane_dim,
        },
        PaneRenderLayout {
            active_pane: layout.active_pane_id()?,
            pane_layout: &pane_layout,
        },
        runtimes,
        terminal_size,
        &attention_panes,
    )?;
    Ok((layout_snapshot, pane_regions, render_composer, render_baseline))
}

fn layout_snapshot_and_persist(
    paths: &SessionPaths,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    tracked_processes: &PaneTrackedProcessSnapshot,
) -> rootcause::Result<LayoutSnapshot> {
    self::layout_snapshot_and_maybe_persist(paths, layout, runtimes, tracked_processes, true)
}

fn layout_snapshot_and_maybe_persist(
    paths: &SessionPaths,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    tracked_processes: &PaneTrackedProcessSnapshot,
    persist_layout: bool,
) -> rootcause::Result<LayoutSnapshot> {
    let synced = runtimes.sync_layout_terminal_titles(layout)?;
    if persist_layout && synced.layout_changed() {
        crate::state::persisted::write_metadata(paths, layout)?;
    }
    let runtime_metadata = PaneRuntimeMetadata::from_sources(
        synced.titles().to_vec(),
        runtimes.startup_cmd_labels(),
        tracked_processes,
    );
    layout.snapshot_with_runtime_metadata(&runtime_metadata.pane_snapshot_fields())
}

fn attach_pane_sinks(
    runtimes: &PaneRuntimes,
    sender: &mpsc::SyncSender<PtyEvent>,
) -> rootcause::Result<Vec<AttachedPtySink>> {
    Ok(runtimes
        .attach_sinks(sender)?
        .into_iter()
        .map(|(pane_id, guard)| AttachedPtySink { guard, pane_id })
        .collect())
}

fn attach_pane_sink(
    runtimes: &PaneRuntimes,
    sender: &mpsc::SyncSender<PtyEvent>,
    pane_id: PaneId,
) -> rootcause::Result<AttachedPtySink> {
    Ok(AttachedPtySink {
        guard: runtimes.handle(pane_id)?.attach_sink(sender.clone())?,
        pane_id,
    })
}

async fn handle_client(
    config: &ServerConfig,
    connection: ServerConnection,
    attach_request: AttachRequest,
    delete_sessions: &DeleteSessions,
    state: &mut SessionRuntimeState,
) -> rootcause::Result<()> {
    self::resize_panes_to_layout(&state.layout, &state.pane_runtimes, &attach_request.terminal_size)?;
    let (pty_event_sender, pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
    let mut sink_guards = self::attach_pane_sinks(&state.pane_runtimes, &pty_event_sender)?;
    let (mut request_reader, mut event_writer) = connection.split();
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    pane_tracked_processes.observe_all_runtime_pane_cmds(
        config.user_config.as_ref(),
        &state.layout,
        &state.pane_runtimes,
        Instant::now(),
    )?;
    let (layout_snapshot, pane_regions, mut render_composer, render_baseline) = self::initial_attached_render(
        config,
        &mut state.layout,
        &state.pane_runtimes,
        &pane_tracked_processes,
        &attach_request.terminal_size,
    )?;
    let last_layout_snapshot = layout_snapshot.clone();
    let attached_pane_regions = pane_regions.clone();
    if !self::send_attached_response_and_baseline(
        &mut event_writer,
        layout_snapshot,
        pane_regions,
        render_baseline,
        config.client_write_timeout,
    )
    .await?
    {
        return Ok(());
    }

    let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
    let bridge_handle = tokio::task::spawn_blocking(move || {
        while let Ok(event) = pty_event_receiver.recv() {
            if async_pty_sender
                .blocking_send(SessionPaneOutputMessage::from(event))
                .is_err()
            {
                break;
            }
        }
    });
    let mut attached_state = AttachedSessionState {
        pane_tracked_processes,
        config,
        delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot,
        layout: &mut state.layout,
        pane_fullscreen: PaneFullscreen::default(),
        pane_regions: attached_pane_regions,
        pty_event_sender: &pty_event_sender,
        render_composer: &mut render_composer,
        runtimes: &mut state.pane_runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size: attach_request.terminal_size,
    };
    let result = self::run_attached_client(
        &mut request_reader,
        &mut event_writer,
        &mut attached_state,
        &mut async_pty_receiver,
    )
    .await;
    let restore_result = self::restore_scrollback_editor_without_render(&mut attached_state);
    drop(attached_state);

    drop(sink_guards);
    drop(pty_event_sender);
    drop(async_pty_receiver);
    bridge_handle
        .await
        .map_err(|error| report!("muxr server pty bridge task panicked").attach(format!("{error}")))?;
    match result {
        Ok(()) => restore_result,
        Err(error) => {
            let _ = restore_result.inspect_err(|restore_error| {
                crate::session_tracing::scrollback::restore_failed(restore_error);
            });
            Err(error)
        }
    }
}

fn pane_regions_snapshot(pane_layout: &PaneLayout, runtimes: &PaneRuntimes) -> rootcause::Result<PaneRegionsSnapshot> {
    let regions = pane_layout
        .regions()
        .iter()
        .map(|region| self::pane_region_snapshot(region, runtimes))
        .collect::<rootcause::Result<Vec<_>>>()?;
    PaneRegionsSnapshot::new(regions)
}

fn pane_region_snapshot(region: &PaneRegion, runtimes: &PaneRuntimes) -> rootcause::Result<PaneRegionSnapshot> {
    let handle = runtimes.handle(region.id)?;
    let mouse_mode = handle.mouse_mode()?;
    let visible_top_row = handle.visible_top_row()?;
    PaneRegionSnapshot::new(
        region.id,
        region.area.origin.col,
        region.area.origin.row,
        region.area.size.cols,
        region.area.size.rows,
        mouse_mode,
        visible_top_row,
    )
}

fn visible_pane_region_at_position(
    state: &AttachedSessionState<'_>,
    position: ClientMousePosition,
) -> rootcause::Result<Option<PaneRegion>> {
    Ok(self::visible_pane_layout(state)?
        .regions()
        .iter()
        .find(|region| region.contains(position.into()))
        .cloned())
}

fn visible_pane_id_at_position(
    state: &AttachedSessionState<'_>,
    position: ClientMousePosition,
) -> rootcause::Result<Option<PaneId>> {
    Ok(self::visible_pane_region_at_position(state, position)?.map(|region| region.id))
}

fn visible_pane_region_snapshot_at_position(
    state: &AttachedSessionState<'_>,
    position: ClientMousePosition,
) -> rootcause::Result<Option<PaneRegionSnapshot>> {
    let Some(region) = self::visible_pane_region_at_position(state, position)? else {
        return Ok(None);
    };
    Ok(Some(self::pane_region_snapshot(&region, state.runtimes)?))
}

fn attention_pane_ids(layout: &SessionLayout, pane_tracked_processes: &PaneTrackedProcesses) -> Vec<PaneId> {
    let mut pane_ids = layout.attention_pane_ids();
    for pane_id in pane_tracked_processes.attention_pane_ids(layout) {
        if !pane_ids.contains(&pane_id) {
            pane_ids.push(pane_id);
        }
    }
    pane_ids
}

async fn send_attached_response_and_baseline(
    event_writer: &mut ServerEventWriter,
    layout: LayoutSnapshot,
    pane_regions: PaneRegionsSnapshot,
    render_baseline: RenderUpdate,
    client_write_timeout: Duration,
) -> rootcause::Result<bool> {
    if !self::send_writer_event_with_timeout(
        event_writer,
        &ServerEvent::Attached(AttachAccepted { layout, pane_regions }),
        client_write_timeout,
    )
    .await?
    {
        return Ok(false);
    }
    self::send_writer_event_with_timeout(
        event_writer,
        &ServerEvent::Render(render_baseline),
        client_write_timeout,
    )
    .await
}

#[expect(
    clippy::too_many_lines,
    reason = "the two biased select branches keep request/output priority ordering explicit"
)]
async fn run_attached_client(
    request_reader: &mut ServerRequestReader,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<SessionPaneOutputMessage>,
) -> rootcause::Result<()> {
    let mut timers = AttachedClientTimers::new(state.config)?;
    timers.sync_tracked_process_quiet_deadline(state.pane_tracked_processes.next_quiet_deadline()?)?;
    let mut heartbeat_started_at: Option<tokio::time::Instant> = None;
    let mut render_dirty = false;
    let mut request_turn = false;

    loop {
        if self::attached_client_should_exit(state, heartbeat_started_at) {
            return Ok(());
        }
        timers.sync_render_deadline(render_dirty)?;

        if request_turn {
            tokio::select! {
                biased;
                _ = timers.heartbeat.tick() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::HeartbeatTick,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat_started_at,
                        &mut render_dirty,
                    ).await? {
                        return Ok(());
                    }
                },
                () = timers.render_sleep.as_mut() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::RenderDeadlineReached,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat_started_at,
                        &mut render_dirty,
                    ).await? {
                        return Ok(());
                    }
                },
                () = timers.cmd_handoff_sample.as_mut() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::CmdHandoffSampleReady,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat_started_at,
                        &mut render_dirty,
                    ).await? {
                        return Ok(());
                    }
                },
                () = timers.tracked_process_quiet_sleep.as_mut() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat_started_at,
                        &mut render_dirty,
                    ).await? {
                        return Ok(());
                    }
                },
                request = request_reader.recv_request() => {
                    request_turn = false;
                    let message = SessionAttachedClientMessage::from_request(request?);
                    if !self::handle_attached_client_message(message, event_writer, state, &mut timers, &mut heartbeat_started_at, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                event = pty_event_receiver.recv() => {
                    request_turn = true;
                    if !self::handle_pane_output_message(event, event_writer, state, &mut timers, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
            }
        } else {
            tokio::select! {
                biased;
                _ = timers.heartbeat.tick() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::HeartbeatTick,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat_started_at,
                        &mut render_dirty,
                    ).await? {
                        return Ok(());
                    }
                },
                () = timers.render_sleep.as_mut() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::RenderDeadlineReached,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat_started_at,
                        &mut render_dirty,
                    ).await? {
                        return Ok(());
                    }
                },
                () = timers.cmd_handoff_sample.as_mut() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::CmdHandoffSampleReady,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat_started_at,
                        &mut render_dirty,
                    ).await? {
                        return Ok(());
                    }
                },
                () = timers.tracked_process_quiet_sleep.as_mut() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat_started_at,
                        &mut render_dirty,
                    ).await? {
                        return Ok(());
                    }
                },
                event = pty_event_receiver.recv() => {
                    // Output gets one turn, then client requests get first chance so detach/pong cannot starve.
                    request_turn = true;
                    if !self::handle_pane_output_message(event, event_writer, state, &mut timers, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                request = request_reader.recv_request() => {
                    request_turn = false;
                    let message = SessionAttachedClientMessage::from_request(request?);
                    if !self::handle_attached_client_message(message, event_writer, state, &mut timers, &mut heartbeat_started_at, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
            }
        }
    }
}

fn attached_client_should_exit(
    state: &AttachedSessionState<'_>,
    heartbeat_started_at: Option<tokio::time::Instant>,
) -> bool {
    // A dropped PTY sink means live output is already stale; release the
    // active slot instead of draining old frames into a slow client.
    if !state.sink_guards.iter().all(|sink| sink.guard.is_output_current()) {
        return true;
    }
    if let Some(started_at) = heartbeat_started_at
        && started_at.elapsed() > state.config.client_heartbeat_timeout
    {
        return true;
    }
    // The delete requester already received the explicit ack; attached clients can observe connection close.
    // Waiting to notify a slow attached terminal would delay server-owned cleanup of the selected session.
    state.delete_sessions.is_requested()
}

async fn handle_session_runtime_timer_message(
    message: SessionRuntimeTimerMessage,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    timers: &mut AttachedClientTimers,
    heartbeat_started_at: &mut Option<tokio::time::Instant>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match message {
        SessionRuntimeTimerMessage::HeartbeatTick => {
            self::send_heartbeat_if_idle(event_writer, state.config.client_write_timeout, heartbeat_started_at).await
        }
        SessionRuntimeTimerMessage::RenderDeadlineReached => {
            let keep_attached = self::flush_render_diff(event_writer, state, render_dirty).await?;
            // `Sleep` stays ready after it fires. Disable the render deadline immediately so an idle attached
            // client cannot hot-spin after consuming the one-shot render wakeup.
            timers.disable_render_sleep()?;
            Ok(keep_attached)
        }
        SessionRuntimeTimerMessage::CmdHandoffSampleReady => {
            self::handle_cmd_handoff_sample(timers, event_writer, state, render_dirty).await
        }
        SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached => {
            timers.disable_tracked_process_quiet_sleep()?;
            if !self::flush_pane_attention(timers, event_writer, state, render_dirty).await? {
                return Ok(false);
            }
            timers.sync_tracked_process_quiet_deadline(state.pane_tracked_processes.next_quiet_deadline()?)?;
            Ok(true)
        }
    }
}

async fn send_heartbeat_if_idle(
    event_writer: &mut ServerEventWriter,
    client_write_timeout: Duration,
    heartbeat_started_at: &mut Option<tokio::time::Instant>,
) -> rootcause::Result<bool> {
    if heartbeat_started_at.is_some() {
        return Ok(true);
    }

    if !self::send_writer_event_with_timeout(event_writer, &ServerEvent::Ping, client_write_timeout).await? {
        return Ok(false);
    }
    *heartbeat_started_at = Some(tokio::time::Instant::now());
    Ok(true)
}

async fn handle_pane_output_message(
    event: Option<SessionPaneOutputMessage>,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    timers: &mut AttachedClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match event {
        Some(SessionPaneOutputMessage::PaneExited) => Ok(!self::handle_reaped_panes(state, event_writer).await?),
        Some(SessionPaneOutputMessage::PaneOutputReady) => {
            let screen_dirty_panes = state.runtimes.take_screen_dirty_panes();
            let title_changes = state.runtimes.take_title_changes()?;
            let screen_dirty = !screen_dirty_panes.is_empty();
            let screen_dirty_visible = self::pane_ids_include_visible(
                state.layout,
                &state.pane_fullscreen,
                &state.terminal_size,
                &screen_dirty_panes,
            )?;
            // PTY output from hidden panes can still update titles/tracked-process state, but it must not make the
            // attached client rebuild the visible frame when the effective pane layout cannot show those cells.
            *render_dirty |= screen_dirty_visible;
            // Start the coalescing window before bounded writer sends below; otherwise slow sends add another frame.
            timers.sync_render_deadline(*render_dirty)?;
            let now = Instant::now();
            let tracked_process_changed = if screen_dirty {
                state.pane_tracked_processes.observe_runtime_visible_activity(
                    state.config.user_config.as_ref(),
                    state.runtimes,
                    &screen_dirty_panes,
                    now,
                )?
            } else {
                false
            };
            if !title_changes.is_empty() && !self::flush_cmd_label_layout(event_writer, state, title_changes).await? {
                return Ok(false);
            }
            if tracked_process_changed
                && !self::flush_tracked_process_runtime_layout(
                    timers,
                    event_writer,
                    state,
                    render_dirty,
                    screen_dirty_visible,
                )
                .await?
            {
                return Ok(false);
            }
            timers.sync_tracked_process_quiet_deadline(state.pane_tracked_processes.next_quiet_deadline()?)?;
            // PTY exit status is sticky state. Detached exits wake the server loop through `pane_exit_notify`; while
            // attached, the bounded output channel is only a wakeup hint, so sweep the sticky state here.
            if self::handle_reaped_panes(state, event_writer).await? {
                return Ok(false);
            }
            Ok(true)
        }
        None => Ok(false),
    }
}

fn pane_ids_include_visible(
    layout: &SessionLayout,
    pane_fullscreen: &PaneFullscreen,
    terminal_size: &TerminalSize,
    pane_ids: &[PaneId],
) -> rootcause::Result<bool> {
    if pane_ids.is_empty() {
        return Ok(false);
    }
    Ok(pane_fullscreen
        .pane_layout(layout, terminal_size)?
        .regions()
        .iter()
        .any(|region| pane_ids.contains(&region.id)))
}

async fn flush_render_diff(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    if !*render_dirty {
        return Ok(true);
    }

    let (pane_regions, render_update) = {
        let pane_layout = self::visible_pane_layout(state)?;
        let pane_regions = self::pane_regions_snapshot(&pane_layout, state.runtimes)?;
        let attention_panes = self::attention_pane_ids(state.layout, &state.pane_tracked_processes);
        let reason = if pane_regions == state.pane_regions {
            RenderDiffReason::DirtyFrame
        } else {
            // Scrollback can move the viewport without changing the visible pixels. Send an empty diff in that case so
            // clients can complete scroll-dependent state after the matching PaneRegions event.
            RenderDiffReason::RegionChanged
        };
        let update = state.render_composer.render_diff(
            PaneRenderConfig {
                border_styles: state.config.user_config.pane_borders,
                mode: crate::keyboard_input::border_render_mode(state.input_mode),
                pane_attention: state.config.user_config.pane_attention,
                pane_dim: state.config.user_config.pane_dim,
            },
            PaneRenderLayout {
                active_pane: state.layout.active_pane_id()?,
                pane_layout: &pane_layout,
            },
            state.runtimes,
            &state.terminal_size,
            &attention_panes,
            reason,
        )?;
        (pane_regions, update)
    };
    if !self::send_pane_regions_and_render(event_writer, state, pane_regions, render_update).await? {
        return Ok(false);
    }
    *render_dirty = false;
    Ok(true)
}

fn runtime_pane_metadata(state: &AttachedSessionState<'_>) -> rootcause::Result<PaneRuntimeMetadata> {
    let terminal_titles = state.runtimes.terminal_titles()?;
    let startup_cmd_labels = state.runtimes.startup_cmd_labels();
    let tracked_processes = state.pane_tracked_processes.snapshot();
    Ok(PaneRuntimeMetadata::from_sources(
        terminal_titles,
        startup_cmd_labels,
        &tracked_processes,
    ))
}

async fn flush_cmd_label_layout(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    title_changes: Vec<(PaneId, Option<String>)>,
) -> rootcause::Result<bool> {
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let changes = {
        let mut last_layout_snapshot = state.last_layout_snapshot.clone();
        let mut layout_changed = false;
        let mut changes = Vec::new();
        for (pane_id, title) in title_changes {
            layout_changed |= state.layout.sync_terminal_titles(&[(pane_id, title.clone())]);
            let runtime_metadata = runtime_metadata.with_terminal_title_override(pane_id, title);
            let layout_snapshot = state
                .layout
                .snapshot_with_runtime_metadata(&runtime_metadata.pane_snapshot_fields())?;
            if layout_snapshot == last_layout_snapshot {
                continue;
            }
            last_layout_snapshot = layout_snapshot.clone();
            changes.push(layout_snapshot);
        }
        if layout_changed && state.scrollback_editor.is_none() {
            crate::state::persisted::write_metadata(&state.config.paths, state.layout)?;
        }
        changes
    };

    for layout_snapshot in changes {
        // Terminal-title changes affect only sidebar metadata; avoid rebuilding the pane frame for cmd/cwd churn.
        if !self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn handle_cmd_handoff_sample(
    timers: &mut AttachedClientTimers,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let pane_ids = timers.take_cmd_handoff_sample_panes()?;
    if pane_ids.is_empty() {
        return Ok(true);
    }

    let changed = state.pane_tracked_processes.observe_runtime_pane_cmds(
        state.config.user_config.as_ref(),
        state.runtimes,
        &pane_ids,
        Instant::now(),
    )?;
    timers.sync_tracked_process_quiet_deadline(state.pane_tracked_processes.next_quiet_deadline()?)?;
    if changed {
        let pane_surface_dirty =
            self::pane_ids_include_visible(state.layout, &state.pane_fullscreen, &state.terminal_size, &pane_ids)?;
        return self::flush_tracked_process_runtime_layout(
            timers,
            event_writer,
            state,
            render_dirty,
            pane_surface_dirty,
        )
        .await;
    }
    Ok(true)
}

async fn flush_tracked_process_runtime_layout(
    timers: &mut AttachedClientTimers,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
    pane_surface_dirty: bool,
) -> rootcause::Result<bool> {
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let layout_snapshot = state
        .layout
        .snapshot_with_runtime_metadata(&runtime_metadata.pane_snapshot_fields())?;
    *render_dirty |= pane_surface_dirty;
    timers.sync_render_deadline(*render_dirty)?;
    self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await
}

async fn flush_pane_attention(
    timers: &mut AttachedClientTimers,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let now = Instant::now();
    let pane_surface_dirty = match state.pane_tracked_processes.mark_quiet_deadlines(state.layout, now)? {
        TrackedProcessAttention::Seen => false,
        TrackedProcessAttention::Unseen { pane_ids } => {
            self::pane_ids_include_visible(state.layout, &state.pane_fullscreen, &state.terminal_size, &pane_ids)?
        }
        TrackedProcessAttention::Unchanged => return Ok(true),
    };
    *render_dirty |= pane_surface_dirty;
    timers.sync_render_deadline(*render_dirty)?;
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let layout_snapshot = state
        .layout
        .snapshot_with_runtime_metadata(&runtime_metadata.pane_snapshot_fields())?;

    self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await
}

async fn send_sidebar_layout_if_changed(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    layout_snapshot: LayoutSnapshot,
) -> rootcause::Result<bool> {
    if layout_snapshot == state.last_layout_snapshot {
        return Ok(true);
    }
    if !self::send_writer_event_with_timeout(
        event_writer,
        &ServerEvent::SidebarLayout(layout_snapshot.clone()),
        state.config.client_write_timeout,
    )
    .await?
    {
        return Ok(false);
    }
    state.last_layout_snapshot = layout_snapshot;
    Ok(true)
}

async fn send_pane_regions_and_render(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    pane_regions: PaneRegionsSnapshot,
    render_update: Option<RenderUpdate>,
) -> rootcause::Result<bool> {
    // Region metadata must precede the render using it: selection/copy translate visible cells through
    // `visible_top_row`, so tab-bar-only renders still need the same ordering as normal pane renders.
    if pane_regions != state.pane_regions {
        if !self::send_writer_event_with_timeout(
            event_writer,
            &ServerEvent::PaneRegions(pane_regions.clone()),
            state.config.client_write_timeout,
        )
        .await?
        {
            return Ok(false);
        }
        state.pane_regions = pane_regions;
    }
    if let Some(render_update) = render_update
        && !self::send_writer_event_with_timeout(
            event_writer,
            &ServerEvent::Render(render_update),
            state.config.client_write_timeout,
        )
        .await?
    {
        return Ok(false);
    }
    Ok(true)
}

async fn send_layout_and_baseline(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    let (layout_snapshot, pane_regions, render_update) = {
        let tracked_processes = state.pane_tracked_processes.snapshot();
        let layout_snapshot = self::layout_snapshot_and_maybe_persist(
            &state.config.paths,
            state.layout,
            state.runtimes,
            &tracked_processes,
            state.scrollback_editor.is_none(),
        )?;
        let pane_layout = self::visible_pane_layout(state)?;
        let pane_regions = self::pane_regions_snapshot(&pane_layout, state.runtimes)?;
        let attention_panes = self::attention_pane_ids(state.layout, &state.pane_tracked_processes);
        let render_update = state.render_composer.render_baseline(
            PaneRenderConfig {
                border_styles: state.config.user_config.pane_borders,
                mode: crate::keyboard_input::border_render_mode(state.input_mode),
                pane_attention: state.config.user_config.pane_attention,
                pane_dim: state.config.user_config.pane_dim,
            },
            PaneRenderLayout {
                active_pane: state.layout.active_pane_id()?,
                pane_layout: &pane_layout,
            },
            state.runtimes,
            &state.terminal_size,
            &attention_panes,
        )?;
        (layout_snapshot, pane_regions, render_update)
    };
    if !self::send_writer_event_with_timeout(
        event_writer,
        &ServerEvent::Layout(layout_snapshot.clone()),
        state.config.client_write_timeout,
    )
    .await?
    {
        return Ok(false);
    }
    if !self::send_writer_event_with_timeout(
        event_writer,
        &ServerEvent::PaneRegions(pane_regions.clone()),
        state.config.client_write_timeout,
    )
    .await?
    {
        return Ok(false);
    }
    state.pane_regions = pane_regions;
    if !self::send_writer_event_with_timeout(
        event_writer,
        &ServerEvent::Render(render_update),
        state.config.client_write_timeout,
    )
    .await?
    {
        return Ok(false);
    }
    state.last_layout_snapshot = layout_snapshot;
    Ok(true)
}

async fn handle_reaped_panes(
    state: &mut AttachedSessionState<'_>,
    event_writer: &mut ServerEventWriter,
) -> rootcause::Result<bool> {
    let previous_pane_before_restore = state.layout.active_pane_id()?;
    let restored_editor = self::restore_scrollback_editor_before_reap_if_needed(state)?;
    let previous_pane_before_reap = state.layout.active_pane_id()?;
    match self::reap_exited_panes(state.config, state.layout, state.runtimes)? {
        ReapResult::Final => Ok(true),
        ReapResult::NoExitedPanes => {
            if !restored_editor {
                return Ok(false);
            }
            self::write_active_pane_focus_events(previous_pane_before_restore, state)?;
            Ok(!self::send_layout_and_baseline(event_writer, state).await?)
        }
        ReapResult::Removed => {
            let live_panes = state.runtimes.pane_ids();
            state.sink_guards.retain(|sink| live_panes.contains(&sink.pane_id));
            let previous_pane = if restored_editor {
                previous_pane_before_restore
            } else {
                previous_pane_before_reap
            };
            self::write_active_pane_focus_events(previous_pane, state)?;
            Ok(!self::resize_panes_and_render(event_writer, state).await?)
        }
    }
}

fn restore_scrollback_editor_before_reap_if_needed(state: &mut AttachedSessionState<'_>) -> rootcause::Result<bool> {
    if state.scrollback_editor.is_none() {
        return Ok(false);
    }
    if state.runtimes.exited_panes()?.is_empty() {
        return Ok(false);
    }
    // Reap only against the real pane tree. The editor tree is attached-client-local; restoring first avoids
    // persisting a temporary `nvim` pane or reaping a hidden original pane against the wrong layout.
    self::write_scrollback_editor_focus_lost_if_live(state.scrollback_editor.as_ref(), state.runtimes)?;
    self::restore_scrollback_editor_without_render(state)?;
    Ok(true)
}

fn restore_scrollback_editor_without_render(state: &mut AttachedSessionState<'_>) -> rootcause::Result<()> {
    let Some(editor) = state.scrollback_editor.take() else {
        return Ok(());
    };
    let editor_pane_id = editor.editor_pane_id();
    crate::scrollback_editor::restore(
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_fullscreen,
        editor,
    )?;
    state.sink_guards.retain(|sink| sink.pane_id != editor_pane_id);
    Ok(())
}

fn write_scrollback_editor_focus_lost_if_live(
    editor: Option<&ScrollbackEditorState>,
    runtimes: &PaneRuntimes,
) -> rootcause::Result<()> {
    let Some(editor) = editor else {
        return Ok(());
    };
    let editor_pane_id = editor.editor_pane_id();
    if !runtimes.pane_ids().contains(&editor_pane_id) {
        return Ok(());
    }
    let handle = runtimes.handle(editor_pane_id)?;
    if !handle.has_exited() {
        // Restore removes the temporary editor runtime, so the editor must receive FocusLost while its PTY is still
        // live; the restored original pane receives FocusGained after restore.
        handle.write_focus_event(TerminalFocusEvent::Lost)?;
    }
    Ok(())
}

const fn scrollback_editor_cmd_action(cmd: ClientCmd, editor_active: bool) -> ScrollbackEditorCmdAction {
    if !editor_active {
        return ScrollbackEditorCmdAction::Run(cmd);
    }
    match cmd {
        ClientCmd::ClosePane => ScrollbackEditorCmdAction::Restore,
        ClientCmd::EnterResizeMode
        | ClientCmd::ExitMode
        | ClientCmd::FocusPane(_)
        | ClientCmd::OpenScrollbackEditor
        | ClientCmd::ResizePane(_)
        | ClientCmd::SplitPane(_)
        | ClientCmd::Tab(_)
        | ClientCmd::TogglePaneFullscreen => ScrollbackEditorCmdAction::Ignore,
    }
}

async fn resize_panes_and_render(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    let pane_layout = self::visible_pane_layout(state)?;
    state.runtimes.resize_panes(pane_layout.regions())?;
    self::send_layout_and_baseline(event_writer, state).await
}

fn visible_pane_layout(state: &AttachedSessionState<'_>) -> rootcause::Result<PaneLayout> {
    state.pane_fullscreen.pane_layout(state.layout, &state.terminal_size)
}

async fn handle_attached_client_message(
    message: SessionAttachedClientMessage,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    timers: &mut AttachedClientTimers,
    heartbeat_started_at: &mut Option<tokio::time::Instant>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match message {
        SessionAttachedClientMessage::ClientDisconnected => Ok(false),
        SessionAttachedClientMessage::Request(request) => {
            self::handle_attached_request(request, event_writer, state, timers, heartbeat_started_at, render_dirty)
                .await
        }
    }
}

async fn handle_attached_request(
    request: ClientRequest,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    timers: &mut AttachedClientTimers,
    heartbeat_started_at: &mut Option<tokio::time::Instant>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match request {
        ClientRequest::Detach => self::send_detached_event(event_writer, state).await,
        ClientRequest::DeleteSession => {
            crate::sessions_delete::handle_attached_delete(
                event_writer,
                state.delete_sessions,
                state.config.client_write_timeout,
            )
            .await?;
            Ok(false)
        }
        ClientRequest::Input(bytes) => self::handle_pane_bytes_request(
            &bytes,
            state,
            timers,
            crate::keyboard_input::input_interaction(&bytes),
            render_dirty,
            PtyHandle::write_input,
        ),
        ClientRequest::Paste(bytes) => {
            // Bracketed paste can contain newlines as data; only raw input newlines mean prompt submission.
            self::handle_pane_bytes_request(
                &bytes,
                state,
                timers,
                TrackedProcessUserInteraction::MayEcho,
                render_dirty,
                PtyHandle::write_paste,
            )
        }
        ClientRequest::Key(key) => self::handle_key_request(key, event_writer, state, timers, render_dirty).await,
        ClientRequest::Mouse(event) => {
            self::handle_mouse_event_request(event, event_writer, state, timers, render_dirty).await
        }
        ClientRequest::ScrollPaneLineAt { position, direction } => {
            self::handle_scroll_pane_line_at_client_request(
                position,
                direction,
                event_writer,
                state,
                timers,
                render_dirty,
            )
            .await
        }
        ClientRequest::FocusPaneAt(position) => {
            // The scrollback editor layout is attached-client-local. Direct mouse focus must not mutate that temporary
            // tree, otherwise subsequent input can move away from the editor pane before restore.
            if state.scrollback_editor.is_some() {
                return Ok(true);
            }
            self::handle_focus_pane_at_client_request(position, event_writer, state).await
        }
        ClientRequest::FocusTab(tab_id) => {
            if state.scrollback_editor.is_some() {
                return Ok(true);
            }
            self::handle_focus_tab_client_request(tab_id, event_writer, state).await
        }
        ClientRequest::Resize(size) => {
            state.terminal_size = size;
            if !self::resize_panes_and_render(event_writer, state).await? {
                return Ok(false);
            }
            Ok(true)
        }
        ClientRequest::RenderResync => {
            if !self::send_layout_and_baseline(event_writer, state).await? {
                return Ok(false);
            }
            Ok(true)
        }
        ClientRequest::Ping => {
            self::send_writer_event_with_timeout(event_writer, &ServerEvent::Pong, state.config.client_write_timeout)
                .await
        }
        ClientRequest::Pong => {
            *heartbeat_started_at = None;
            Ok(true)
        }
        request @ ClientRequest::Attach(_) => {
            let _sent = self::send_writer_event_with_timeout(
                event_writer,
                &ServerEvent::Error(ServerError::unexpected_request(request)),
                state.config.client_write_timeout,
            )
            .await?;
            Ok(false)
        }
    }
}

async fn handle_open_scrollback_editor_request(
    dump_style: ScrollbackDumpStyle,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    if state.scrollback_editor.is_some() {
        return Ok(true);
    }
    let previous_pane = state.layout.active_pane_id()?;
    state.input_mode = ServerInputMode::Normal;
    let opened = crate::scrollback_editor::open(
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_fullscreen,
        &state.terminal_size,
        dump_style,
    )?;
    let editor_pane_id = opened.state.editor_pane_id();
    let sink = match self::attach_pane_sink(state.runtimes, state.pty_event_sender, editor_pane_id) {
        Ok(sink) => sink,
        Err(error) => {
            crate::scrollback_editor::restore(
                state.config,
                state.layout,
                state.runtimes,
                &mut state.pane_fullscreen,
                opened.state,
            )?;
            return Err(error);
        }
    };
    state.sink_guards.push(sink);
    state.scrollback_editor = Some(opened.state);
    self::write_active_pane_focus_events(previous_pane, state)?;
    self::resize_panes_and_render(event_writer, state).await
}

fn handle_pane_bytes_request(
    bytes: &[u8],
    state: &mut AttachedSessionState<'_>,
    timers: &mut AttachedClientTimers,
    interaction: TrackedProcessUserInteraction,
    render_dirty: &mut bool,
    write: impl FnOnce(&PtyHandle, &[u8]) -> rootcause::Result<bool>,
) -> rootcause::Result<bool> {
    if !bytes.is_empty() {
        *render_dirty |= self::write_active_pane_user_input(state, timers, interaction, |handle| write(handle, bytes))?;
        timers.sync_render_deadline(*render_dirty)?;
    }
    Ok(true)
}

async fn handle_scroll_pane_line_at_client_request(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    event_writer: &mut ServerEventWriter,
    state: &AttachedSessionState<'_>,
    timers: &mut AttachedClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let scrolled = if let Some(pane_id) = self::visible_pane_id_at_position(state, position)? {
        crate::pane_scroll::scroll_pane(pane_id, direction, PaneScrollAmount::Line, state.runtimes)?
    } else {
        false
    };
    let outcome = crate::pane_scroll::scroll_pane_line_result(position, direction, scrolled);
    *render_dirty |= outcome.render_dirty;
    timers.sync_render_deadline(*render_dirty)?;
    self::send_writer_event_with_timeout(event_writer, &outcome.event, state.config.client_write_timeout).await
}

async fn handle_focus_pane_at_client_request(
    position: ClientMousePosition,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    if state.pane_fullscreen.visible_pane_id(state.layout)?.is_some() {
        return Ok(true);
    }
    let previous_pane = state.layout.active_pane_id()?;
    if !crate::pane_focus::handle_focus_pane_at_request_with_tracked_process_ack(
        position,
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_tracked_processes,
        &state.terminal_size,
        Instant::now(),
    )? {
        return Ok(true);
    }
    self::write_active_pane_focus_events(previous_pane, state)?;
    self::send_layout_and_baseline(event_writer, state).await
}

async fn handle_focus_tab_client_request(
    tab_id: TabId,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    let previous_pane = state.layout.active_pane_id()?;
    if !crate::tab_focus::handle_focus_tab_request_with_tracked_process_ack(
        tab_id,
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_tracked_processes,
        Instant::now(),
    )? {
        return Ok(true);
    }
    self::write_active_pane_focus_events(previous_pane, state)?;
    self::resize_panes_and_render(event_writer, state).await
}

async fn send_detached_event(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    self::restore_scrollback_editor_without_render(state)?;
    self::record_detach_ack_send_failure(
        self::send_writer_event_failure(event_writer, &ServerEvent::Detached, state.config.client_write_timeout).await,
    );
    Ok(false)
}

fn record_detach_ack_send_failure(reason: Option<ClientEventSendFailure>) {
    if let Some(reason) = reason {
        crate::session_tracing::ack::detach_failed(reason);
    }
}

async fn handle_key_request(
    key: ClientKey,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    timers: &mut AttachedClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match crate::keyboard_input::resolve_key(&mut state.input_mode, &key) {
        KeyResolution::Cmd(cmd) => self::handle_cmd_request(cmd, event_writer, state).await,
        KeyResolution::Raw => {
            if !key.raw_bytes.is_empty() {
                *render_dirty |= self::write_active_pane_user_input(
                    state,
                    timers,
                    crate::keyboard_input::input_interaction(&key.raw_bytes),
                    |handle| handle.write_input(&key.raw_bytes),
                )?;
                timers.sync_render_deadline(*render_dirty)?;
            }
            Ok(true)
        }
    }
}

async fn handle_cmd_request(
    cmd: ClientCmd,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    let cmd = match self::scrollback_editor_cmd_action(cmd, state.scrollback_editor.is_some()) {
        ScrollbackEditorCmdAction::Ignore => {
            // The editor pane is attached-client-local. Muxr layout shortcuts are blocked while it is active so they
            // cannot create temporary panes/runtimes that disappear from the restored real layout.
            return Ok(true);
        }
        ScrollbackEditorCmdAction::Restore => {
            let previous_pane = state.layout.active_pane_id()?;
            self::write_scrollback_editor_focus_lost_if_live(state.scrollback_editor.as_ref(), state.runtimes)?;
            self::restore_scrollback_editor_without_render(state)?;
            self::write_active_pane_focus_events(previous_pane, state)?;
            return self::resize_panes_and_render(event_writer, state).await;
        }
        ScrollbackEditorCmdAction::Run(cmd) => cmd,
    };
    match cmd {
        ClientCmd::Tab(cmd) => self::handle_tab_cmd_request(cmd, event_writer, state).await,
        ClientCmd::SplitPane(split_axis) => {
            let previous_pane = state.layout.active_pane_id()?;
            state.pane_fullscreen.clear_active_tab(state.layout);
            let pane_id = crate::pane_split::handle_split_pane_cmd(
                split_axis,
                state.config,
                state.layout,
                state.runtimes,
                &state.terminal_size,
            )?;
            state
                .sink_guards
                .push(self::attach_pane_sink(state.runtimes, state.pty_event_sender, pane_id)?);
            self::write_active_pane_focus_events(previous_pane, state)?;
            self::resize_panes_and_render(event_writer, state).await
        }
        ClientCmd::ClosePane => {
            let previous_pane = state.layout.active_pane_id()?;
            state.pane_fullscreen.clear_active_tab(state.layout);
            let outcome = crate::pane_close::handle_close_pane_cmd(state.config, state.layout, state.runtimes)?;
            match &outcome {
                ClosePaneOutcome::Final { pane_id } | ClosePaneOutcome::Removed { pane_id } => {
                    state.sink_guards.retain(|sink| &sink.pane_id != pane_id);
                }
            }
            match outcome {
                ClosePaneOutcome::Final { .. } => self::send_detached_event(event_writer, state).await,
                ClosePaneOutcome::Removed { .. } => {
                    self::write_active_pane_focus_events(previous_pane, state)?;
                    self::resize_panes_and_render(event_writer, state).await
                }
            }
        }
        ClientCmd::ResizePane(direction) => {
            if !crate::pane_resize::handle_resize_pane_cmd(direction, state.config, state.layout)? {
                return Ok(true);
            }
            self::resize_panes_and_render(event_writer, state).await
        }
        ClientCmd::OpenScrollbackEditor => {
            self::handle_open_scrollback_editor_request(
                state.config.user_config.scrollback.dump_style,
                event_writer,
                state,
            )
            .await
        }
        ClientCmd::FocusPane(direction) => {
            let previous_pane = state.layout.active_pane_id()?;
            if !crate::pane_focus::handle_focus_pane_cmd_with_tracked_process_ack(
                direction,
                state.config,
                state.layout,
                state.runtimes,
                &mut state.pane_tracked_processes,
                &state.terminal_size,
                Instant::now(),
            )? {
                return Ok(true);
            }
            self::write_active_pane_focus_events(previous_pane, state)?;
            if state.pane_fullscreen.clear_active_tab(state.layout) {
                return self::resize_panes_and_render(event_writer, state).await;
            }
            self::send_layout_and_baseline(event_writer, state).await
        }
        ClientCmd::EnterResizeMode => {
            state.pane_fullscreen.clear_active_tab(state.layout);
            self::resize_panes_and_render(event_writer, state).await
        }
        ClientCmd::ExitMode => self::send_layout_and_baseline(event_writer, state).await,
        ClientCmd::TogglePaneFullscreen => {
            state.pane_fullscreen.toggle_active_pane(state.layout)?;
            self::resize_panes_and_render(event_writer, state).await
        }
    }
}

async fn handle_tab_cmd_request(
    cmd: TabCmd,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    let previous_pane = state.layout.active_pane_id()?;
    let focus_may_change = matches!(cmd, TabCmd::Create | TabCmd::FocusPrevious | TabCmd::FocusNext);
    match cmd {
        TabCmd::Create => {
            let pane_id = crate::tab_create::handle_create_tab_cmd(
                state.config,
                state.layout,
                state.runtimes,
                &state.terminal_size,
            )?;
            state
                .sink_guards
                .push(self::attach_pane_sink(state.runtimes, state.pty_event_sender, pane_id)?);
        }
        TabCmd::FocusPrevious => {
            crate::tab_focus::handle_focus_previous_tab_cmd_with_tracked_process_ack(
                state.config,
                state.layout,
                state.runtimes,
                &mut state.pane_tracked_processes,
                Instant::now(),
            )?;
        }
        TabCmd::FocusNext => {
            crate::tab_focus::handle_focus_next_tab_cmd_with_tracked_process_ack(
                state.config,
                state.layout,
                state.runtimes,
                &mut state.pane_tracked_processes,
                Instant::now(),
            )?;
        }
        TabCmd::MovePrevious => {
            crate::tab_move::handle_move_active_tab_previous_cmd(state.config, state.layout)?;
        }
        TabCmd::MoveNext => {
            crate::tab_move::handle_move_active_tab_next_cmd(state.config, state.layout)?;
        }
    }
    if focus_may_change {
        self::write_active_pane_focus_events(previous_pane, state)?;
    }
    self::resize_panes_and_render(event_writer, state).await
}

fn active_pane_handle_with_id(
    layout: &SessionLayout,
    runtimes: &PaneRuntimes,
) -> rootcause::Result<(PaneId, PtyHandle)> {
    let active_pane = layout.active_pane_id()?;
    let handle = runtimes.handle(active_pane)?;
    Ok((active_pane, handle))
}

fn write_active_pane_focus_events(previous_pane: PaneId, state: &AttachedSessionState<'_>) -> rootcause::Result<()> {
    let next_pane = state.layout.active_pane_id()?;
    self::write_pane_focus_transition(previous_pane, next_pane, state.runtimes)
}

fn write_pane_focus_transition(
    previous_pane: PaneId,
    next_pane: PaneId,
    runtimes: &PaneRuntimes,
) -> rootcause::Result<()> {
    // Focus reporting is a pane-application opt-in (`CSI ? 1004 h`). Close/reap may remove the old pane runtime
    // before the new pane receives focus, so skip missing runtimes while still notifying the surviving side.
    for (pane_id, event) in self::pane_focus_events_for_live_panes(previous_pane, next_pane, &runtimes.pane_ids()) {
        runtimes.handle(pane_id)?.write_focus_event(event)?;
    }
    Ok(())
}

fn pane_focus_events_for_live_panes(
    previous_pane: PaneId,
    next_pane: PaneId,
    live_panes: &[PaneId],
) -> Vec<(PaneId, TerminalFocusEvent)> {
    if previous_pane == next_pane {
        return Vec::new();
    }

    let mut events = Vec::with_capacity(2);
    if live_panes.contains(&previous_pane) {
        events.push((previous_pane, TerminalFocusEvent::Lost));
    }
    if live_panes.contains(&next_pane) {
        events.push((next_pane, TerminalFocusEvent::Gained));
    }
    events
}

fn write_active_pane_user_input(
    state: &mut AttachedSessionState<'_>,
    timers: &mut AttachedClientTimers,
    interaction: TrackedProcessUserInteraction,
    write: impl FnOnce(&PtyHandle) -> rootcause::Result<bool>,
) -> rootcause::Result<bool> {
    let (pane_id, handle) = self::active_pane_handle_with_id(state.layout, state.runtimes)?;
    let render_dirty = write(&handle)?;
    state
        .pane_tracked_processes
        .record_user_interaction(pane_id, interaction, Instant::now());
    if interaction == TrackedProcessUserInteraction::StartsTrackedProcessWork {
        timers.schedule_cmd_handoff_sample(pane_id)?;
    }
    Ok(render_dirty)
}

async fn handle_mouse_event_request(
    event: ClientMouseEvent,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    timers: &mut AttachedClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let Some(region) = self::visible_pane_region_snapshot_at_position(state, event.position)? else {
        return Ok(true);
    };
    let handle = state.runtimes.handle(*region.id())?;
    let action = crate::pane_mouse::resolve_pane_mouse_action(event, handle.application_mode()?);
    match action {
        crate::pane_mouse::PaneMouseAction::ForwardToPty { focus, protocol } => {
            // Focus-reporting apps must observe the pane transition before the click bytes; only the layout render can
            // wait until after forwarding the mouse packet.
            let focused_pane = if focus.focuses_pane() {
                self::focus_pane_at_mouse_event(event, state)?
            } else {
                false
            };
            if let Some(scrolled_to_bottom) = handle.write_mouse_event(event, &region, protocol)? {
                *render_dirty |= scrolled_to_bottom;
                timers.sync_render_deadline(*render_dirty)?;
                state.pane_tracked_processes.record_user_interaction(
                    *region.id(),
                    TrackedProcessUserInteraction::MayEcho,
                    Instant::now(),
                );
            }
            if focused_pane {
                return self::send_layout_and_baseline(event_writer, state).await;
            }
            Ok(true)
        }
        crate::pane_mouse::PaneMouseAction::FauxScrollPty {
            cursor_key_mode,
            direction,
        } => {
            *render_dirty |= handle.write_faux_scroll_input(direction, cursor_key_mode)?;
            timers.sync_render_deadline(*render_dirty)?;
            state.pane_tracked_processes.record_user_interaction(
                *region.id(),
                TrackedProcessUserInteraction::MayEcho,
                Instant::now(),
            );
            Ok(true)
        }
        crate::pane_mouse::PaneMouseAction::ScrollHistory { direction } => {
            let Some(pane_id) = self::visible_pane_id_at_position(state, event.position)? else {
                return Ok(true);
            };
            if !crate::pane_scroll::scroll_pane(pane_id, direction, PaneScrollAmount::Wheel, state.runtimes)? {
                return Ok(true);
            }
            // Wheel input can arrive much faster than render IO; mark dirty and let the render deadline coalesce.
            *render_dirty = true;
            timers.sync_render_deadline(*render_dirty)?;
            Ok(true)
        }
        crate::pane_mouse::PaneMouseAction::FocusPane => self::handle_mouse_focus(event, event_writer, state).await,
        crate::pane_mouse::PaneMouseAction::NoAction => Ok(true),
    }
}

async fn handle_mouse_focus(
    event: ClientMouseEvent,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    if !self::focus_pane_at_mouse_event(event, state)? {
        return Ok(true);
    }
    self::send_layout_and_baseline(event_writer, state).await
}

fn focus_pane_at_mouse_event(event: ClientMouseEvent, state: &mut AttachedSessionState<'_>) -> rootcause::Result<bool> {
    if state.scrollback_editor.is_some() {
        return Ok(false);
    }
    if state.pane_fullscreen.visible_pane_id(state.layout)?.is_some() {
        return Ok(false);
    }
    let previous_pane = state.layout.active_pane_id()?;
    if !crate::pane_focus::handle_focus_pane_at_request_with_tracked_process_ack(
        event.position,
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_tracked_processes,
        &state.terminal_size,
        Instant::now(),
    )? {
        return Ok(false);
    }
    self::write_active_pane_focus_events(previous_pane, state)?;
    Ok(true)
}

/// Send one event on an attached-client writer with the server's bounded write timeout.
///
/// # Errors
/// This function currently returns `Ok(false)` for send failures and timeouts instead of an error.
async fn send_writer_event_with_timeout(
    writer: &mut ServerEventWriter,
    event: &ServerEvent,
    client_write_timeout: Duration,
) -> rootcause::Result<bool> {
    Ok(self::send_writer_event_failure(writer, event, client_write_timeout)
        .await
        .is_none())
}

async fn send_writer_event_failure(
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::thread;

    use muxr_config::MuxrConfig;
    use muxr_core::SessionName;
    use muxr_core::SessionPaths;
    use muxr_transport::ClientConnection;
    use muxr_transport::ServerListener;
    use rstest::rstest;

    use super::*;
    use crate::pane_cmd::PaneCmd;
    use crate::pane_cmd::PaneCmdObservation;
    use crate::pane_focus::PaneFocusDirection;
    use crate::pane_runtime::test_helpers as pane_runtime_test_helpers;
    use crate::pane_split::PaneSplitAxis;
    use crate::server::test_helpers as server_test_helpers;
    use crate::session_start_seed::SessionStartSeed;
    use crate::state::SessionMetadata;

    #[test]
    fn test_layout_snapshot_and_persist_when_runtime_cmd_exists_sets_snapshot_cmd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&session, self::metadata("zsh", 1))?;
        let runtimes = pane_runtime_test_helpers::empty_runtimes();
        let pane_id = PaneId::new(1)?;
        let mut tracked_processes = PaneTrackedProcesses::default();
        assert2::assert!(tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &PaneCmdObservation::FgCmd {
                cmd: PaneCmd {
                    executable: "codex".to_owned(),
                    path: None,
                    pid: 42,
                },
            },
            Instant::now(),
        ));

        let snapshot =
            self::layout_snapshot_and_persist(&paths, &mut layout, &runtimes, &tracked_processes.snapshot())?;

        let pane = snapshot
            .tabs()
            .first()
            .and_then(|tab| tab.panes().first())
            .ok_or_else(|| report!("expected pane snapshot"))?;
        pretty_assertions::assert_eq!(pane.cmd_label, Some("cx".to_owned()));
        Ok(())
    }

    #[test]
    fn test_record_detach_ack_send_failure_when_reason_exists_warns() -> rootcause::Result<()> {
        let session = SessionName::default();

        let log = crate::session_tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            self::record_detach_ack_send_failure(Some(ClientEventSendFailure::SendFailed));
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
        let log = crate::session_tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            self::record_detach_ack_send_failure(None);
            Ok(())
        })?;

        assert2::assert!(!log.contains("kind=\"detach_ack_send_failed\""));
        Ok(())
    }

    #[test]
    fn test_pane_ids_include_visible_when_pane_is_in_inactive_tab_returns_false() -> rootcause::Result<()> {
        let session = SessionName::default();
        let mut layout = SessionLayout::initial(&session, self::metadata("sh", 1))?;
        let inactive_pane = PaneId::new(1)?;
        let active_pane = layout.create_tab(self::metadata("sh", 2))?;
        let fullscreen = PaneFullscreen::default();
        let terminal_size = TerminalSize::new(80, 24)?;

        assert2::assert!(!self::pane_ids_include_visible(
            &layout,
            &fullscreen,
            &terminal_size,
            &[inactive_pane]
        )?);
        assert2::assert!(self::pane_ids_include_visible(
            &layout,
            &fullscreen,
            &terminal_size,
            &[active_pane]
        )?);
        assert2::assert!(self::pane_ids_include_visible(
            &layout,
            &fullscreen,
            &terminal_size,
            &[inactive_pane, active_pane],
        )?);
        assert2::assert!(!self::pane_ids_include_visible(
            &layout,
            &fullscreen,
            &terminal_size,
            &[]
        )?);
        Ok(())
    }

    #[test]
    fn test_pane_ids_include_visible_when_pane_is_hidden_by_fullscreen_returns_false() -> rootcause::Result<()> {
        let session = SessionName::default();
        let mut layout = SessionLayout::initial(&session, self::metadata("sh", 1))?;
        let hidden_pane = PaneId::new(1)?;
        let visible_pane = layout.split_active_pane(
            MuxrConfig::default().layout,
            self::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        let mut fullscreen = PaneFullscreen::default();
        fullscreen.toggle_active_pane(&layout)?;
        let terminal_size = TerminalSize::new(80, 24)?;

        assert2::assert!(!self::pane_ids_include_visible(
            &layout,
            &fullscreen,
            &terminal_size,
            &[hidden_pane]
        )?);
        assert2::assert!(self::pane_ids_include_visible(
            &layout,
            &fullscreen,
            &terminal_size,
            &[visible_pane]
        )?);
        assert2::assert!(self::pane_ids_include_visible(
            &layout,
            &fullscreen,
            &terminal_size,
            &[hidden_pane, visible_pane],
        )?);
        Ok(())
    }

    #[test]
    fn test_run_attached_client_when_completion_channel_full_warns_with_session_span() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let tempdir = tempfile::tempdir()?;
        let config = server_test_helpers::server_config(tempdir.path(), "work")?;
        let session = config.session.clone();

        let log = crate::session_tracing::collect_test_log(&session, || {
            runtime.block_on(async {
                let terminal_size = TerminalSize::new(80, 24)?;
                let attach_request = AttachRequest {
                    session: config.session.clone(),
                    terminal_size,
                };

                crate::session_files::prepare_session_dirs(&config.paths)?;
                let mut session_runtime = SessionRuntime::spawn(
                    &config,
                    &attach_request.terminal_size,
                    Arc::new(tokio::sync::Notify::new()),
                )?;
                let (completion_sender, _completion_receiver) = tokio::sync::mpsc::channel(1);
                let blocked_state = SessionRuntimeState {
                    layout: SessionLayout::initial(&config.session, self::metadata("sh", 1))?,
                    pane_runtimes: pane_runtime_test_helpers::empty_runtimes(),
                };
                completion_sender
                    .send(SessionAttachedClientTaskMessage::Finished(blocked_state))
                    .await
                    .map_err(|_| report!("failed to pre-fill attached-client completion channel"))?;
                let task_runtime = session_runtime
                    .attached_client_task_runtime(completion_sender, Arc::new(DeleteSessions::default()))?;
                let listener = ServerListener::bind(&config.paths.socket)?;
                let (mut client_connection, server_connection) =
                    tokio::try_join!(ClientConnection::connect(&config.paths.socket), listener.accept())?;

                let attached_client = task_runtime.run_attached_client(&config, server_connection, attach_request);
                let detached_client = async {
                    client_connection.send_request(&ClientRequest::Detach).await?;
                    self::read_connection_until_detached(&mut client_connection).await
                };
                let (attached_client_result, detached_client_result) = tokio::join!(attached_client, detached_client);
                attached_client_result?;
                detached_client_result?;
                Ok(())
            })
        })?;

        assert2::assert!(log.contains("kind=\"attached_client_state_handoff_failed\""));
        assert2::assert!(log.contains("reason=\"channel_full\""));
        assert2::assert!(log.contains("session=work"));
        Ok(())
    }

    #[rstest]
    #[case::inactive_runs(
        ClientCmd::SplitPane(PaneSplitAxis::Vertical),
        false,
        ScrollbackEditorCmdAction::Run(ClientCmd::SplitPane(PaneSplitAxis::Vertical))
    )]
    #[case::active_close_restores(ClientCmd::ClosePane, true, ScrollbackEditorCmdAction::Restore)]
    #[case::active_split_is_ignored(
        ClientCmd::SplitPane(PaneSplitAxis::Vertical),
        true,
        ScrollbackEditorCmdAction::Ignore
    )]
    #[case::active_create_tab_is_ignored(ClientCmd::Tab(TabCmd::Create), true, ScrollbackEditorCmdAction::Ignore)]
    #[case::active_open_scrollback_editor_is_ignored(
        ClientCmd::OpenScrollbackEditor,
        true,
        ScrollbackEditorCmdAction::Ignore
    )]
    #[case::active_focus_pane_is_ignored(
        ClientCmd::FocusPane(PaneFocusDirection::Right),
        true,
        ScrollbackEditorCmdAction::Ignore
    )]
    fn test_scrollback_editor_cmd_action_when_editor_mode_is_active_blocks_layout_mutations(
        #[case] cmd: ClientCmd,
        #[case] editor_active: bool,
        #[case] expected: ScrollbackEditorCmdAction,
    ) {
        pretty_assertions::assert_eq!(self::scrollback_editor_cmd_action(cmd, editor_active), expected);
    }

    #[test]
    fn test_write_scrollback_editor_focus_lost_if_live_when_editor_is_reporting_writes_lost() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = server_test_helpers::server_config(tempdir.path(), "work")?;
        let mut user_config = MuxrConfig::default();
        user_config.scrollback.editor = muxr_config::ScrollbackEditorConfig {
            program: "/bin/sh",
            args: &[
                "-c",
                "printf '\\033[?1004hready\\n'; \
                 stty raw -echo; \
                 dd bs=3 count=1 2>/dev/null | od -An -tx1 -v; \
                 sleep 30",
                "muxr-test-scrollback-editor",
            ],
        };
        config.user_config = Arc::new(user_config);
        config.shell_cmd = server_test_helpers::shell_cmd_with_args("/bin/sh", &["-c", "sleep 30"]);
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: Vec::new(),
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        let mut pane_fullscreen = PaneFullscreen::default();
        let opened = crate::scrollback_editor::open(
            &config,
            &mut layout,
            &mut runtimes,
            &mut pane_fullscreen,
            &terminal_size,
            config.user_config.scrollback.dump_style,
        )?;
        let editor_pane_id = opened.state.editor_pane_id();
        self::wait_for_runtime_snapshot_contains(&runtimes, editor_pane_id, "ready")?;

        self::write_scrollback_editor_focus_lost_if_live(Some(&opened.state), &runtimes)?;

        self::wait_for_runtime_snapshot_contains(&runtimes, editor_pane_id, "1b 5b 4f")?;
        Ok(())
    }

    #[test]
    fn test_pane_focus_events_for_live_panes_when_runtime_sets_vary_returns_focus_transition() -> rootcause::Result<()>
    {
        let previous_pane = PaneId::new(1)?;
        let next_pane = PaneId::new(2)?;

        for (previous_pane, next_pane, live_panes, expected) in [
            (
                previous_pane,
                next_pane,
                vec![previous_pane, next_pane],
                vec![
                    (previous_pane, TerminalFocusEvent::Lost),
                    (next_pane, TerminalFocusEvent::Gained),
                ],
            ),
            (previous_pane, previous_pane, vec![previous_pane], Vec::new()),
            (
                previous_pane,
                next_pane,
                vec![next_pane],
                vec![(next_pane, TerminalFocusEvent::Gained)],
            ),
            (
                previous_pane,
                next_pane,
                vec![previous_pane],
                vec![(previous_pane, TerminalFocusEvent::Lost)],
            ),
            (previous_pane, next_pane, Vec::new(), Vec::new()),
        ] {
            pretty_assertions::assert_eq!(
                self::pane_focus_events_for_live_panes(previous_pane, next_pane, &live_panes),
                expected,
            );
        }
        Ok(())
    }

    fn wait_for_runtime_snapshot_contains(
        runtimes: &PaneRuntimes,
        pane_id: PaneId,
        needle: &str,
    ) -> rootcause::Result<()> {
        let started_at = Instant::now();
        loop {
            let snapshot = runtimes.handle(pane_id)?.render_snapshot()?;
            let rendered = snapshot
                .rows()
                .iter()
                .flat_map(|row| row.cells().iter().map(muxr_core::RenderCell::text))
                .collect::<String>();
            if self::snapshot_contains(&rendered, needle) {
                return Ok(());
            }
            if started_at.elapsed() > Duration::from_secs(2) {
                return Err(report!("timed out waiting for muxr runtime snapshot").attach(rendered));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn snapshot_contains(rendered: &str, needle: &str) -> bool {
        if rendered.contains(needle) {
            return true;
        }
        let needle_tokens = needle.split_whitespace().collect::<Vec<_>>();
        let rendered_tokens = rendered.split_whitespace().collect::<Vec<_>>();
        rendered_tokens
            .windows(needle_tokens.len())
            .any(|window| window == needle_tokens.as_slice())
    }

    async fn read_connection_until_detached(connection: &mut ClientConnection) -> rootcause::Result<()> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > Duration::from_secs(2) {
                return Err(report!("timed out waiting for muxr detach ack"));
            }

            match tokio::time::timeout(Duration::from_millis(50), connection.recv_event()).await {
                Ok(Ok(Some(ServerEvent::Detached))) => return Ok(()),
                Ok(Ok(Some(ServerEvent::Ping))) => connection.send_request(&ClientRequest::Pong).await?,
                Ok(Ok(Some(ServerEvent::Error(error)))) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                Ok(Ok(Some(
                    ServerEvent::Attached(_)
                    | ServerEvent::Deleted
                    | ServerEvent::Pong
                    | ServerEvent::Layout(_)
                    | ServerEvent::SidebarLayout(_)
                    | ServerEvent::PaneRegions(_)
                    | ServerEvent::Render(_)
                    | ServerEvent::ScrollPaneLineResult { .. },
                )))
                | Err(_) => {}
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) => return Err(report!("expected detached event")),
            }
        }
    }

    fn session_paths(base: &Path, raw: &str) -> rootcause::Result<(SessionName, SessionPaths)> {
        let session = raw.parse()?;
        let state_root = base.join("muxr");
        let root = state_root.join("sessions").join(raw);

        Ok((
            session,
            SessionPaths {
                socket: state_root.join("s").join(format!("{raw}.sock")),
                pid: root.join("server.pid"),
                layout: root.join("layout.json"),
                panes: root.join("panes"),
                root,
            },
        ))
    }

    fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }
}
