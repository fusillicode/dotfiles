use std::collections::BTreeSet;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

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

use crate::attached_client_timers::AttachedClientTimers;
use crate::cwd_git_stats::CwdGitStats;
use crate::cwd_git_stats::CwdGitStatsRequester;
use crate::cwd_git_stats::CwdGitStatsResult;
use crate::keyboard_input::ClientCmd;
use crate::keyboard_input::KeyResolution;
use crate::keyboard_input::ServerInputMode;
use crate::keyboard_input::TabCmd;
use crate::pane_close::ClosePaneOutcome;
use crate::pane_close::PaneExitOutcome;
use crate::pane_render::PaneRenderConfig;
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
use crate::server::ServerConfig;
use crate::session_runtime::PANE_OUTPUT_EVENT_CHANNEL_LIMIT;
use crate::session_runtime::SessionAttachedClientMessage;
use crate::session_runtime::SessionAttachedClientTaskMessage;
use crate::session_runtime::SessionPaneOutputMessage;
use crate::session_runtime::SessionRuntime;
use crate::session_runtime::SessionRuntimeState;
use crate::session_runtime::SessionRuntimeTimerMessage;
use crate::sessions_delete::DeleteSessions;
use crate::state::SessionLayout;
use crate::terminal::TerminalTitleEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReapResult {
    Final,
    NoExitedPanes,
    Removed,
}

struct AttachedPtySink {
    guard: PtySinkGuard,
    pane_id: PaneId,
}

struct AttachedSessionState<'a> {
    pane_tracked_processes: PaneTrackedProcesses,
    config: &'a ServerConfig,
    delete_sessions: &'a DeleteSessions,
    cwd_git_stats_requester: CwdGitStatsRequester,
    cwd_git_stats: CwdGitStats,
    input_mode: ServerInputMode,
    last_layout_snapshot: LayoutSnapshot,
    layout: &'a mut SessionLayout,
    pane_regions: PaneRegionsSnapshot,
    pty_event_sender: &'a mpsc::SyncSender<PtyEvent>,
    render_composer: &'a mut RenderComposer,
    runtimes: &'a mut PaneRuntimes,
    sink_guards: &'a mut Vec<AttachedPtySink>,
    terminal_size: TerminalSize,
}

pub struct AttachedClientTaskRuntime {
    completion_sender: tokio::sync::mpsc::Sender<SessionAttachedClientTaskMessage>,
    delete_sessions: std::sync::Arc<DeleteSessions>,
    state: SessionRuntimeState,
}

impl AttachedClientTaskRuntime {
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
        let _sent = self
            .completion_sender
            .try_send(SessionAttachedClientTaskMessage::Finished(self.state));
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
    cwd_git_stats: &CwdGitStats,
    terminal_size: &TerminalSize,
) -> rootcause::Result<(LayoutSnapshot, PaneRegionsSnapshot, RenderComposer, RenderUpdate)> {
    let mut render_composer = RenderComposer::default();
    let tracked_processes = pane_tracked_processes.snapshot();
    let (runtime_metadata, _changed_panes) =
        self::synced_runtime_metadata_and_persist(&config.paths, layout, runtimes, &tracked_processes)?;
    let layout_snapshot = self::layout_snapshot_from_runtime_metadata(layout, &runtime_metadata, cwd_git_stats)?;
    let pane_regions = self::pane_regions_snapshot(layout, runtimes, terminal_size)?;
    let attention_panes = self::attention_pane_ids(layout, pane_tracked_processes);
    let render_baseline = render_composer.render_baseline(
        PaneRenderConfig {
            border_styles: config.user_config.pane_borders,
            mode: crate::pane_borders::BorderRenderMode::Focus,
            pane_attention: config.user_config.pane_attention,
            pane_dim: config.user_config.pane_dim,
        },
        layout,
        runtimes,
        terminal_size,
        &attention_panes,
    )?;
    Ok((layout_snapshot, pane_regions, render_composer, render_baseline))
}

fn synced_runtime_metadata_and_persist(
    paths: &SessionPaths,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    tracked_processes: &PaneTrackedProcessSnapshot,
) -> rootcause::Result<(PaneRuntimeMetadata, Vec<PaneId>)> {
    let synced = runtimes.sync_layout_terminal_titles(layout)?;
    if synced.layout_changed() {
        crate::state::persisted::write_metadata(paths, layout)?;
    }
    let runtime_metadata = PaneRuntimeMetadata::from_sources(
        synced.titles().to_vec(),
        runtimes.startup_cmd_labels(),
        tracked_processes,
    );
    Ok((runtime_metadata, synced.changed_panes().to_vec()))
}

fn layout_snapshot_from_runtime_metadata(
    layout: &SessionLayout,
    runtime_metadata: &PaneRuntimeMetadata,
    cwd_git_stats: &CwdGitStats,
) -> rootcause::Result<LayoutSnapshot> {
    let mut snapshot_fields = runtime_metadata.pane_snapshot_fields();
    cwd_git_stats.populate_snapshot_fields(layout, &mut snapshot_fields);
    layout.snapshot_with_runtime_metadata(&snapshot_fields)
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
    let cwd_git_stats = CwdGitStats::default();
    let (cwd_git_stats_requester, mut git_stats_result_receiver) = crate::cwd_git_stats::cwd_git_stats_worker();
    let (layout_snapshot, pane_regions, mut render_composer, render_baseline) = self::initial_attached_render(
        config,
        &mut state.layout,
        &state.pane_runtimes,
        &pane_tracked_processes,
        &cwd_git_stats,
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
        cwd_git_stats_requester,
        cwd_git_stats,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot,
        layout: &mut state.layout,
        pane_regions: attached_pane_regions,
        pty_event_sender: &pty_event_sender,
        render_composer: &mut render_composer,
        runtimes: &mut state.pane_runtimes,
        sink_guards: &mut sink_guards,
        terminal_size: attach_request.terminal_size,
    };
    let runtime_metadata = self::runtime_pane_metadata(&attached_state)?;
    let pane_ids = attached_state
        .layout
        .panes()
        .into_iter()
        .map(|pane| pane.id)
        .collect::<Vec<_>>();
    self::request_cwd_git_stats_for_runtime_metadata(&mut attached_state, &runtime_metadata, pane_ids)?;
    let result = self::run_attached_client(
        &mut request_reader,
        &mut event_writer,
        &mut attached_state,
        &mut async_pty_receiver,
        &mut git_stats_result_receiver,
    )
    .await;

    drop(sink_guards);
    drop(pty_event_sender);
    drop(async_pty_receiver);
    bridge_handle
        .await
        .map_err(|error| report!("muxr server pty bridge task panicked").attach(format!("{error}")))?;
    result
}

fn pane_regions_snapshot(
    layout: &SessionLayout,
    runtimes: &PaneRuntimes,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneRegionsSnapshot> {
    let regions = layout
        .pane_regions(terminal_size)?
        .into_iter()
        .map(|region| {
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
        })
        .collect::<rootcause::Result<Vec<_>>>()?;
    PaneRegionsSnapshot::new(regions)
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
    git_stats_result_receiver: &mut tokio::sync::mpsc::Receiver<Vec<CwdGitStatsResult>>,
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
                _ = timers.shell_poll.tick() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::ShellPollTick,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat_started_at,
                        &mut render_dirty,
                    ).await? {
                        return Ok(());
                    }
                },
                _ = timers.render_tick.tick() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::RenderTick,
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
                git_stats_results = git_stats_result_receiver.recv() => {
                    if !self::handle_git_stats_results(git_stats_results, event_writer, state).await? {
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
                _ = timers.shell_poll.tick() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::ShellPollTick,
                        event_writer,
                        state,
                        &mut timers,
                        &mut heartbeat_started_at,
                        &mut render_dirty,
                    ).await? {
                        return Ok(());
                    }
                },
                _ = timers.render_tick.tick() => {
                    if !self::handle_session_runtime_timer_message(
                        SessionRuntimeTimerMessage::RenderTick,
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
                git_stats_results = git_stats_result_receiver.recv() => {
                    if !self::handle_git_stats_results(git_stats_results, event_writer, state).await? {
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
        SessionRuntimeTimerMessage::ShellPollTick => Ok(!self::handle_reaped_panes(state, event_writer).await?),
        SessionRuntimeTimerMessage::RenderTick => self::flush_render_diff(event_writer, state, render_dirty).await,
        SessionRuntimeTimerMessage::CmdHandoffSampleReady => {
            self::handle_cmd_handoff_sample(timers, event_writer, state, render_dirty).await
        }
        SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached => {
            timers.disable_tracked_process_quiet_sleep()?;
            if !self::flush_pane_attention(event_writer, state, render_dirty).await? {
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
            let title_events = state.runtimes.take_title_events()?;
            let screen_dirty = !screen_dirty_panes.is_empty();
            *render_dirty |= screen_dirty;
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
            if !title_events.is_empty() && !self::flush_cmd_label_layout(event_writer, state, title_events).await? {
                return Ok(false);
            }
            if tracked_process_changed
                && !self::flush_tracked_process_runtime_layout(event_writer, state, render_dirty).await?
            {
                return Ok(false);
            }
            timers.sync_tracked_process_quiet_deadline(state.pane_tracked_processes.next_quiet_deadline()?)?;
            Ok(true)
        }
        None => Ok(false),
    }
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
        let pane_regions = self::pane_regions_snapshot(state.layout, state.runtimes, &state.terminal_size)?;
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
            state.layout,
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

fn request_cwd_git_stats_for_runtime_metadata(
    state: &mut AttachedSessionState<'_>,
    runtime_metadata: &PaneRuntimeMetadata,
    pane_ids: impl IntoIterator<Item = PaneId>,
) -> rootcause::Result<()> {
    let snapshot_fields = runtime_metadata.pane_snapshot_fields();
    let refreshes = state
        .cwd_git_stats
        .prepare_refreshes(state.layout, &snapshot_fields, pane_ids);
    state.cwd_git_stats_requester.request_refreshes(refreshes)
}

async fn handle_git_stats_results(
    results: Option<Vec<CwdGitStatsResult>>,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    let Some(results) = results else {
        return Ok(false);
    };
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let snapshot_fields = runtime_metadata.pane_snapshot_fields();
    let changed_panes = state
        .cwd_git_stats
        .apply_results(state.layout, &snapshot_fields, results);
    if changed_panes.is_empty() {
        return Ok(true);
    }
    let layout_snapshot =
        self::layout_snapshot_from_runtime_metadata(state.layout, &runtime_metadata, &state.cwd_git_stats)?;
    self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await
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
    title_events: Vec<(PaneId, TerminalTitleEvent)>,
) -> rootcause::Result<bool> {
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let changes = {
        let mut last_layout_snapshot = state.last_layout_snapshot.clone();
        let mut layout_changed = false;
        let mut changes = Vec::new();
        for (pane_id, event) in title_events {
            let title = event.into_title();
            let title_event = [(pane_id, title.clone())];
            let changed_panes = state.layout.sync_terminal_titles(&title_event);
            let prompt_refreshes = state
                .cwd_git_stats
                .take_ready_submit_refreshes(state.layout, &title_event);
            let runtime_metadata = runtime_metadata.with_terminal_title_override(pane_id, title);
            if !changed_panes.is_empty() {
                layout_changed = true;
                state
                    .cwd_git_stats
                    .clear_pending_submit_cwds_for_panes(changed_panes.iter().copied());
            }
            let refresh_panes = changed_panes.into_iter().collect::<BTreeSet<_>>();
            if !refresh_panes.is_empty() {
                self::request_cwd_git_stats_for_runtime_metadata(state, &runtime_metadata, refresh_panes)?;
            }
            state.cwd_git_stats_requester.request_refreshes(prompt_refreshes)?;
            let layout_snapshot =
                self::layout_snapshot_from_runtime_metadata(state.layout, &runtime_metadata, &state.cwd_git_stats)?;
            if layout_snapshot == last_layout_snapshot {
                continue;
            }
            last_layout_snapshot = layout_snapshot.clone();
            changes.push(layout_snapshot);
        }
        if layout_changed {
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
        return self::flush_tracked_process_runtime_layout(event_writer, state, render_dirty).await;
    }
    Ok(true)
}

async fn flush_tracked_process_runtime_layout(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let layout_snapshot =
        self::layout_snapshot_from_runtime_metadata(state.layout, &runtime_metadata, &state.cwd_git_stats)?;
    *render_dirty = true;
    self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await
}

async fn flush_pane_attention(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let now = Instant::now();
    let unseen_panes = match state.pane_tracked_processes.mark_quiet_deadlines(state.layout, now)? {
        TrackedProcessAttention::Seen => Vec::new(),
        TrackedProcessAttention::Unchanged => return Ok(true),
        TrackedProcessAttention::Unseen { pane_ids } => pane_ids,
    };
    *render_dirty = true;
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    if !unseen_panes.is_empty() {
        self::request_cwd_git_stats_for_runtime_metadata(state, &runtime_metadata, unseen_panes)?;
    }
    let layout_snapshot =
        self::layout_snapshot_from_runtime_metadata(state.layout, &runtime_metadata, &state.cwd_git_stats)?;

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
        let (runtime_metadata, changed_panes) = self::synced_runtime_metadata_and_persist(
            &state.config.paths,
            state.layout,
            state.runtimes,
            &tracked_processes,
        )?;
        // Baseline sync can observe the post-command cwd before the queued title event is drained. Keep pending
        // command-submit refreshes until the title-event path consumes them, so shared-repo sibling panes refresh the
        // repo where Enter was pressed even if the submitting pane has already moved elsewhere.
        self::request_cwd_git_stats_for_runtime_metadata(state, &runtime_metadata, changed_panes)?;
        let layout_snapshot =
            self::layout_snapshot_from_runtime_metadata(state.layout, &runtime_metadata, &state.cwd_git_stats)?;
        let pane_regions = self::pane_regions_snapshot(state.layout, state.runtimes, &state.terminal_size)?;
        let attention_panes = self::attention_pane_ids(state.layout, &state.pane_tracked_processes);
        let render_update = state.render_composer.render_baseline(
            PaneRenderConfig {
                border_styles: state.config.user_config.pane_borders,
                mode: crate::keyboard_input::border_render_mode(state.input_mode),
                pane_attention: state.config.user_config.pane_attention,
                pane_dim: state.config.user_config.pane_dim,
            },
            state.layout,
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
    match self::reap_exited_panes(state.config, state.layout, state.runtimes)? {
        ReapResult::Final => Ok(true),
        ReapResult::NoExitedPanes => Ok(false),
        ReapResult::Removed => {
            let live_panes = state.runtimes.pane_ids();
            state.sink_guards.retain(|sink| live_panes.contains(&sink.pane_id));
            Ok(!self::resize_panes_and_render(event_writer, state).await?)
        }
    }
}

async fn resize_panes_and_render(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    self::resize_panes_to_layout(state.layout, state.runtimes, &state.terminal_size)?;
    self::send_layout_and_baseline(event_writer, state).await
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
        ClientRequest::Mouse(event) => self::handle_mouse_event_request(event, event_writer, state, render_dirty).await,
        ClientRequest::ScrollPaneLineAt { position, direction } => {
            self::handle_scroll_pane_line_at_client_request(position, direction, event_writer, state, render_dirty)
                .await
        }
        ClientRequest::FocusPaneAt(position) => {
            self::handle_focus_pane_at_client_request(position, event_writer, state).await
        }
        ClientRequest::FocusTab(tab_id) => self::handle_focus_tab_client_request(tab_id, event_writer, state).await,
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
    }
    Ok(true)
}

async fn handle_scroll_pane_line_at_client_request(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    event_writer: &mut ServerEventWriter,
    state: &AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let outcome = crate::pane_scroll::handle_scroll_pane_line_request(
        position,
        direction,
        state.layout,
        state.runtimes,
        &state.terminal_size,
    )?;
    *render_dirty |= outcome.render_dirty;
    self::send_writer_event_with_timeout(event_writer, &outcome.event, state.config.client_write_timeout).await
}

async fn handle_focus_pane_at_client_request(
    position: ClientMousePosition,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
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
    self::send_layout_and_baseline(event_writer, state).await
}

async fn handle_focus_tab_client_request(
    tab_id: TabId,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
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
    self::send_layout_and_baseline(event_writer, state).await
}

async fn send_detached_event(
    event_writer: &mut ServerEventWriter,
    state: &AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    let _sent =
        self::send_writer_event_with_timeout(event_writer, &ServerEvent::Detached, state.config.client_write_timeout)
            .await?;
    Ok(false)
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
    match cmd {
        ClientCmd::Tab(cmd) => self::handle_tab_cmd_request(cmd, event_writer, state).await,
        ClientCmd::SplitPane(split_axis) => {
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
            let runtime_metadata = self::runtime_pane_metadata(state)?;
            self::request_cwd_git_stats_for_runtime_metadata(state, &runtime_metadata, [pane_id])?;
            self::resize_panes_and_render(event_writer, state).await
        }
        ClientCmd::ClosePane => {
            let outcome = crate::pane_close::handle_close_pane_cmd(state.config, state.layout, state.runtimes)?;
            match &outcome {
                ClosePaneOutcome::Final { pane_id } | ClosePaneOutcome::Removed { pane_id } => {
                    state.sink_guards.retain(|sink| &sink.pane_id != pane_id);
                }
            }
            match outcome {
                ClosePaneOutcome::Final { .. } => self::send_detached_event(event_writer, state).await,
                ClosePaneOutcome::Removed { .. } => self::resize_panes_and_render(event_writer, state).await,
            }
        }
        ClientCmd::ResizePane(direction) => {
            if !crate::pane_resize::handle_resize_pane_cmd(direction, state.config, state.layout)? {
                return Ok(true);
            }
            self::resize_panes_and_render(event_writer, state).await
        }
        ClientCmd::FocusPane(direction) => {
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
            self::send_layout_and_baseline(event_writer, state).await
        }
        ClientCmd::EnterResizeMode | ClientCmd::ExitMode => self::send_layout_and_baseline(event_writer, state).await,
    }
}

async fn handle_tab_cmd_request(
    cmd: TabCmd,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
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
            let runtime_metadata = self::runtime_pane_metadata(state)?;
            self::request_cwd_git_stats_for_runtime_metadata(state, &runtime_metadata, [pane_id])?;
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

fn write_active_pane_user_input(
    state: &mut AttachedSessionState<'_>,
    timers: &mut AttachedClientTimers,
    interaction: TrackedProcessUserInteraction,
    write: impl FnOnce(&PtyHandle) -> rootcause::Result<bool>,
) -> rootcause::Result<bool> {
    let (pane_id, handle) = self::active_pane_handle_with_id(state.layout, state.runtimes)?;
    let shell_submit_cwd = if interaction == TrackedProcessUserInteraction::StartsTrackedProcessWork {
        let application_mode = handle.application_mode()?;
        if application_mode.alternate_screen {
            None
        } else {
            let terminal_title = handle.terminal_title()?;
            Some(
                state
                    .layout
                    .pane(pane_id)
                    .ok_or_else(|| rootcause::report!("muxr active pane is missing from server layout"))?
                    .runtime_cwd(terminal_title.as_deref()),
            )
        }
    } else {
        None
    };
    let render_dirty = write(&handle)?;
    state
        .pane_tracked_processes
        .record_user_interaction(pane_id, interaction, Instant::now());
    if interaction == TrackedProcessUserInteraction::StartsTrackedProcessWork {
        timers.schedule_cmd_handoff_sample(pane_id)?;
        if let Some(cwd) = shell_submit_cwd {
            // Git stats follow shell completion, not an arbitrary timeout. Shell prompts report cwd through title
            // updates, so the pending refresh uses the cwd where Enter was pressed even if the command also runs `cd`.
            state.cwd_git_stats.mark_shell_submit_cwd(pane_id, cwd);
        }
    }
    Ok(render_dirty)
}

async fn handle_mouse_event_request(
    event: ClientMouseEvent,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let Some(region) =
        crate::pane_focus::mouse_event_region(state.layout, state.runtimes, &state.terminal_size, event.position)?
    else {
        return Ok(true);
    };
    let handle = state.runtimes.handle(*region.id())?;
    let action = crate::pane_mouse::resolve_pane_mouse_action(event, handle.application_mode()?);
    match action {
        crate::pane_mouse::PaneMouseAction::ForwardToPty { focus, protocol } => {
            if let Some(scrolled_to_bottom) = handle.write_mouse_event(event, &region, protocol)? {
                *render_dirty |= scrolled_to_bottom;
                state.pane_tracked_processes.record_user_interaction(
                    *region.id(),
                    TrackedProcessUserInteraction::MayEcho,
                    Instant::now(),
                );
            }
            if !focus.focuses_pane() {
                return Ok(true);
            }
            self::handle_mouse_focus(event, event_writer, state).await
        }
        crate::pane_mouse::PaneMouseAction::FauxScrollPty {
            application_cursor,
            direction,
        } => {
            *render_dirty |= handle.write_faux_scroll_input(direction, application_cursor)?;
            state.pane_tracked_processes.record_user_interaction(
                *region.id(),
                TrackedProcessUserInteraction::MayEcho,
                Instant::now(),
            );
            Ok(true)
        }
        crate::pane_mouse::PaneMouseAction::ScrollHistory { direction } => {
            if !crate::pane_scroll::handle_scroll_pane_at_request(
                event.position,
                direction,
                PaneScrollAmount::Wheel,
                state.layout,
                state.runtimes,
                &state.terminal_size,
            )? {
                return Ok(true);
            }
            // Wheel input can arrive much faster than render IO; mark dirty and let the normal render tick coalesce.
            *render_dirty = true;
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
    if !crate::pane_focus::handle_focus_pane_at_request_with_tracked_process_ack(
        event.position,
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_tracked_processes,
        &state.terminal_size,
        Instant::now(),
    )? {
        return Ok(true);
    }
    self::send_layout_and_baseline(event_writer, state).await
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
    match tokio::time::timeout(client_write_timeout, writer.send_event(event)).await {
        Ok(Ok(())) => Ok(true),
        Ok(Err(_)) | Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use muxr_config::MuxrConfig;
    use muxr_core::SessionName;
    use muxr_core::SessionPaths;

    use super::*;
    use crate::pane_cmd::PaneCmd;
    use crate::pane_cmd::PaneCmdObservation;
    use crate::pane_runtime::test_helpers as pane_runtime_test_helpers;
    use crate::state::SessionMetadata;

    #[test]
    fn test_synced_runtime_metadata_when_runtime_cmd_exists_sets_snapshot_cmd() -> rootcause::Result<()> {
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

        let (runtime_metadata, _changed_panes) =
            self::synced_runtime_metadata_and_persist(&paths, &mut layout, &runtimes, &tracked_processes.snapshot())?;
        let snapshot =
            self::layout_snapshot_from_runtime_metadata(&layout, &runtime_metadata, &CwdGitStats::default())?;

        let pane = snapshot
            .tabs()
            .first()
            .and_then(|tab| tab.panes().first())
            .ok_or_else(|| report!("expected pane snapshot"))?;
        pretty_assertions::assert_eq!(pane.cmd_label, Some("cx".to_owned()));
        Ok(())
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
