use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
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

use crate::keyboard_input::ClientCmd;
use crate::keyboard_input::KeyResolution;
use crate::keyboard_input::ServerInputMode;
use crate::keyboard_input::TabCmd;
use crate::pane_agent::PaneAgentDetection;
use crate::pane_agent::PaneAgentDetectionWorker;
use crate::pane_agent::PaneAgents;
use crate::pane_agent::PaneUserInteraction;
use crate::pane_close::ClosePaneOutcome;
use crate::pane_close::PaneExitOutcome;
use crate::pane_render::RenderComposer;
use crate::pane_render::RenderDiffReason;
use crate::pane_runtime::PaneRuntimes;
use crate::pane_scroll::PaneScrollAmount;
use crate::pty::PtyEvent;
use crate::pty::PtyHandle;
use crate::pty::PtySinkGuard;
use crate::server::ServerConfig;
use crate::sessions_delete::DeleteSessions;
use crate::state::SessionLayout;

const CLIENT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENT_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const OUTPUT_EVENT_CHANNEL_LIMIT: usize = 1024;
const RENDER_FRAME_INTERVAL: Duration = Duration::from_millis(16);

struct ClientSlotGuard<'a> {
    active_client: &'a AtomicBool,
}

impl Drop for ClientSlotGuard<'_> {
    fn drop(&mut self) {
        self.active_client.store(false, Ordering::Release);
    }
}

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
    detected_agents: Vec<PaneAgentDetection>,
    detected_agents_refreshed_at: Option<Instant>,
    agent_detection_worker: PaneAgentDetectionWorker,
    pane_agents: PaneAgents,
    config: &'a ServerConfig,
    delete_sessions: &'a DeleteSessions,
    input_mode: ServerInputMode,
    last_layout_snapshot: LayoutSnapshot,
    layout: &'a Mutex<SessionLayout>,
    pane_regions: PaneRegionsSnapshot,
    pending_visible_activity_panes: BTreeSet<PaneId>,
    pty_event_sender: &'a mpsc::SyncSender<PtyEvent>,
    render_composer: &'a mut RenderComposer,
    runtimes: &'a Mutex<PaneRuntimes>,
    sink_guards: &'a mut Vec<AttachedPtySink>,
    terminal_size: TerminalSize,
}

struct AttachedClientTimers {
    attention_tick: tokio::time::Interval,
    heartbeat: tokio::time::Interval,
    render_tick: tokio::time::Interval,
    shell_poll: tokio::time::Interval,
}

impl AttachedClientTimers {
    fn new(config: &ServerConfig) -> rootcause::Result<Self> {
        let heartbeat_start = tokio::time::Instant::now()
            .checked_add(config.client_heartbeat_interval)
            .ok_or_else(|| report!("muxr heartbeat interval overflowed"))?;
        let render_start = tokio::time::Instant::now()
            .checked_add(RENDER_FRAME_INTERVAL)
            .ok_or_else(|| report!("muxr render frame interval overflowed"))?;
        let attention_start = tokio::time::Instant::now()
            .checked_add(crate::pane_agent::AGENT_ATTENTION_POLL_INTERVAL)
            .ok_or_else(|| report!("muxr attention poll interval overflowed"))?;

        Ok(Self {
            attention_tick: tokio::time::interval_at(attention_start, crate::pane_agent::AGENT_ATTENTION_POLL_INTERVAL),
            heartbeat: tokio::time::interval_at(heartbeat_start, config.client_heartbeat_interval),
            render_tick: tokio::time::interval_at(render_start, RENDER_FRAME_INTERVAL),
            shell_poll: tokio::time::interval(CLIENT_EVENT_POLL_INTERVAL),
        })
    }
}

struct RuntimePaneMetadata {
    runtime_cmd_labels: Vec<(PaneId, Option<String>)>,
    terminal_titles: Vec<(PaneId, Option<String>)>,
}

pub fn resize_panes_to_layout(
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    size: &TerminalSize,
) -> rootcause::Result<()> {
    let regions = {
        let layout = crate::server::lock_mutex(layout, "layout")?;
        layout.pane_regions(size)?
    };
    let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
    runtimes.resize_panes(&regions)
}

pub fn reap_exited_panes(
    paths: &SessionPaths,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<ReapResult> {
    let exited_panes = {
        let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
        runtimes.exited_panes()?
    };
    if exited_panes.is_empty() {
        return Ok(ReapResult::NoExitedPanes);
    }

    let exited_at = crate::server::unix_timestamp_millis()?;
    let mut result = ReapResult::Removed;
    {
        let mut layout = crate::server::lock_mutex(layout, "layout")?;
        {
            let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
            let _ = runtimes.sync_layout_terminal_titles(&mut layout)?;
        }
        let mut removed_panes = Vec::new();
        for (pane_id, exit_status) in &exited_panes {
            match layout.remove_exited_pane(*pane_id, exited_at, exit_status.clone())? {
                PaneExitOutcome::Final => result = ReapResult::Final,
                PaneExitOutcome::Removed => {}
            }
            removed_panes.push(pane_id);
        }
        crate::state::persisted::write_metadata(paths, &layout)?;
        drop(layout);

        let mut runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
        for pane_id in removed_panes {
            runtimes.remove(*pane_id);
        }
        drop(runtimes);
    }

    Ok(result)
}

pub fn spawn_client_task(
    config: &ServerConfig,
    active_client: &Arc<AtomicBool>,
    delete_sessions: &Arc<DeleteSessions>,
    layout: &Arc<Mutex<SessionLayout>>,
    runtimes: &Arc<Mutex<PaneRuntimes>>,
    connection: ServerConnection,
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) {
    let active_client = Arc::clone(active_client);
    let delete_sessions = Arc::clone(delete_sessions);
    let config = config.clone();
    let layout = Arc::clone(layout);
    let runtimes = Arc::clone(runtimes);
    handles.push(tokio::spawn(async move {
        self::handle_client(
            &config,
            connection,
            &active_client,
            &delete_sessions,
            &layout,
            &runtimes,
        )
        .await
    }));
}

pub fn initial_attached_render(
    paths: &SessionPaths,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<(LayoutSnapshot, PaneRegionsSnapshot, RenderComposer, RenderUpdate)> {
    let mut render_composer = RenderComposer::default();
    let mut layout = crate::server::lock_mutex(layout, "layout")?;
    let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
    let layout_snapshot = self::layout_snapshot_and_persist(paths, &mut layout, &runtimes, &[], &[])?;
    let pane_regions = self::pane_regions_snapshot(&layout, &runtimes, terminal_size)?;
    let attention_panes = layout.attention_pane_ids();
    let render_baseline = render_composer.render_baseline(
        &layout,
        &runtimes,
        terminal_size,
        &attention_panes,
        crate::pane_borders::BorderRenderMode::Focus,
    )?;
    drop(runtimes);
    drop(layout);
    Ok((layout_snapshot, pane_regions, render_composer, render_baseline))
}

pub async fn join_client_tasks(handles: Vec<tokio::task::JoinHandle<rootcause::Result<()>>>) -> rootcause::Result<()> {
    for handle in handles {
        self::join_client_task(handle).await?;
    }
    Ok(())
}

pub async fn join_finished_client_tasks(
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) -> rootcause::Result<()> {
    let mut pending_handles = Vec::new();
    for handle in handles.drain(..) {
        if handle.is_finished() {
            self::join_client_task(handle).await?;
        } else {
            pending_handles.push(handle);
        }
    }
    *handles = pending_handles;
    Ok(())
}

fn layout_snapshot_and_persist(
    paths: &SessionPaths,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    runtime_cmd_labels: &[(PaneId, Option<String>)],
    runtime_agent_states: &[(PaneId, muxr_core::PaneAgentState)],
) -> rootcause::Result<LayoutSnapshot> {
    let synced = runtimes.sync_layout_terminal_titles(layout)?;
    if synced.layout_changed() {
        crate::state::persisted::write_metadata(paths, layout)?;
    }
    layout.snapshot_with_runtime_metadata(synced.titles(), runtime_cmd_labels, runtime_agent_states)
}

fn attach_pane_sinks(
    runtimes: &Mutex<PaneRuntimes>,
    sender: &mpsc::SyncSender<PtyEvent>,
) -> rootcause::Result<Vec<AttachedPtySink>> {
    let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
    Ok(runtimes
        .attach_sinks(sender)?
        .into_iter()
        .map(|(pane_id, guard)| AttachedPtySink { guard, pane_id })
        .collect())
}

fn attach_pane_sink(
    runtimes: &Mutex<PaneRuntimes>,
    sender: &mpsc::SyncSender<PtyEvent>,
    pane_id: PaneId,
) -> rootcause::Result<AttachedPtySink> {
    let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
    Ok(AttachedPtySink {
        guard: runtimes.handle(pane_id)?.attach_sink(sender.clone())?,
        pane_id,
    })
}

async fn handle_client(
    config: &ServerConfig,
    mut connection: ServerConnection,
    active_client: &AtomicBool,
    delete_sessions: &DeleteSessions,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<()> {
    let Some(attach_request) =
        self::handle_client_handshake(&mut connection, delete_sessions, config.client_write_timeout).await?
    else {
        return Ok(());
    };

    if active_client.swap(true, Ordering::AcqRel) {
        let _sent = self::send_connection_event_with_timeout(
            &mut connection,
            &ServerEvent::Error(ServerError::ClientAlreadyAttached),
            config.client_write_timeout,
        )
        .await?;
        return Ok(());
    }
    let _client_slot_guard = ClientSlotGuard { active_client };

    if attach_request.session != config.session {
        let _sent = self::send_connection_event_with_timeout(
            &mut connection,
            &ServerEvent::Error(ServerError::SessionMismatch {
                expected: config.session.clone(),
                actual: attach_request.session.clone(),
            }),
            config.client_write_timeout,
        )
        .await?;
        return Ok(());
    }

    self::resize_panes_to_layout(layout, runtimes, &attach_request.terminal_size)?;
    let (pty_event_sender, pty_event_receiver) = mpsc::sync_channel(OUTPUT_EVENT_CHANNEL_LIMIT);
    let mut sink_guards = self::attach_pane_sinks(runtimes, &pty_event_sender)?;
    let (mut request_reader, mut event_writer) = connection.split();
    let (layout_snapshot, pane_regions, mut render_composer, render_baseline) =
        self::initial_attached_render(&config.paths, layout, runtimes, &attach_request.terminal_size)?;
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

    let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(OUTPUT_EVENT_CHANNEL_LIMIT);
    let bridge_handle = tokio::task::spawn_blocking(move || {
        while let Ok(event) = pty_event_receiver.recv() {
            if async_pty_sender.blocking_send(event).is_err() {
                break;
            }
        }
    });
    let mut attached_state = AttachedSessionState {
        detected_agents: Vec::new(),
        detected_agents_refreshed_at: None,
        agent_detection_worker: PaneAgentDetectionWorker::default(),
        pane_agents: PaneAgents::default(),
        config,
        delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot,
        layout,
        pane_regions: attached_pane_regions,
        pending_visible_activity_panes: BTreeSet::new(),
        pty_event_sender: &pty_event_sender,
        render_composer: &mut render_composer,
        runtimes,
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

    drop(sink_guards);
    drop(pty_event_sender);
    drop(async_pty_receiver);
    bridge_handle
        .await
        .map_err(|error| report!("muxr server pty bridge task panicked").attach(format!("{error}")))?;
    result
}

async fn handle_client_handshake(
    connection: &mut ServerConnection,
    delete_sessions: &DeleteSessions,
    client_write_timeout: Duration,
) -> rootcause::Result<Option<AttachRequest>> {
    let Ok(Ok(Some(request))) = tokio::time::timeout(CLIENT_HANDSHAKE_TIMEOUT, connection.recv_request()).await else {
        return Ok(None);
    };

    match request {
        ClientRequest::DeleteSession => {
            crate::sessions_delete::handle_handshake_delete(connection, delete_sessions, client_write_timeout).await?;
            Ok(None)
        }
        ClientRequest::Ping => {
            let _sent =
                self::send_connection_event_with_timeout(connection, &ServerEvent::Pong, client_write_timeout).await?;
            Ok(None)
        }
        ClientRequest::Attach(attach_request) => Ok(Some(attach_request)),
        request @ (ClientRequest::Pong
        | ClientRequest::Detach
        | ClientRequest::RenderResync
        | ClientRequest::Resize(_)
        | ClientRequest::Input(_)
        | ClientRequest::Paste(_)
        | ClientRequest::Key(_)
        | ClientRequest::Mouse(_)
        | ClientRequest::ScrollPaneLineAt { .. }
        | ClientRequest::FocusPaneAt(_)
        | ClientRequest::FocusTab(_)) => {
            let _sent = self::send_connection_event_with_timeout(
                connection,
                &ServerEvent::Error(ServerError::unexpected_request(request)),
                client_write_timeout,
            )
            .await?;
            Ok(None)
        }
    }
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

fn attention_pane_ids(layout: &SessionLayout, pane_agents: &PaneAgents) -> Vec<PaneId> {
    let mut pane_ids = layout.attention_pane_ids();
    for pane_id in pane_agents.attention_pane_ids(layout) {
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

async fn run_attached_client(
    request_reader: &mut ServerRequestReader,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<PtyEvent>,
) -> rootcause::Result<()> {
    let mut timers = AttachedClientTimers::new(state.config)?;
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
                    if !self::send_heartbeat_if_idle(
                        event_writer,
                        state.config.client_write_timeout,
                        &mut heartbeat_started_at,
                    )
                    .await?
                    {
                        return Ok(());
                    }
                },
                _ = timers.shell_poll.tick() => {
                    if self::handle_reaped_panes(state, event_writer).await? {
                        return Ok(());
                    }
                },
                _ = timers.render_tick.tick() => {
                    if !self::flush_render_diff(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                _ = timers.attention_tick.tick() => {
                    if !self::flush_pane_attention(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                request = request_reader.recv_request() => {
                    request_turn = false;
                    if !self::handle_attached_request(request?, event_writer, state, &mut heartbeat_started_at, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                event = pty_event_receiver.recv() => {
                    request_turn = true;
                    if !self::handle_pty_event(event, event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
            }
        } else {
            tokio::select! {
                biased;
                _ = timers.heartbeat.tick() => {
                    if !self::send_heartbeat_if_idle(
                        event_writer,
                        state.config.client_write_timeout,
                        &mut heartbeat_started_at,
                    )
                    .await?
                    {
                        return Ok(());
                    }
                },
                _ = timers.shell_poll.tick() => {
                    if self::handle_reaped_panes(state, event_writer).await? {
                        return Ok(());
                    }
                },
                _ = timers.render_tick.tick() => {
                    if !self::flush_render_diff(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                _ = timers.attention_tick.tick() => {
                    if !self::flush_pane_attention(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                event = pty_event_receiver.recv() => {
                    // Output gets one turn, then client requests get first chance so detach/pong cannot starve.
                    request_turn = true;
                    if !self::handle_pty_event(event, event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                request = request_reader.recv_request() => {
                    request_turn = false;
                    if !self::handle_attached_request(request?, event_writer, state, &mut heartbeat_started_at, &mut render_dirty).await? {
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

async fn handle_pty_event(
    event: Option<PtyEvent>,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match event {
        Some(PtyEvent::Exited) => Ok(!self::handle_reaped_panes(state, event_writer).await?),
        Some(PtyEvent::OutputReady) => {
            let (screen_dirty_panes, title_changes) = {
                let runtimes = crate::server::lock_mutex(state.runtimes, "pane runtimes")?;
                (runtimes.take_screen_dirty_panes(), runtimes.take_title_changes()?)
            };
            let screen_dirty = !screen_dirty_panes.is_empty();
            *render_dirty |= screen_dirty;
            if !title_changes.is_empty() && !self::flush_cmd_label_layout(event_writer, state, title_changes).await? {
                return Ok(false);
            }
            if screen_dirty {
                state.pending_visible_activity_panes.extend(screen_dirty_panes);
            }
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
        let layout = crate::server::lock_mutex(state.layout, "layout")?;
        let runtimes = crate::server::lock_mutex(state.runtimes, "pane runtimes")?;
        let pane_regions = self::pane_regions_snapshot(&layout, &runtimes, &state.terminal_size)?;
        let attention_panes = self::attention_pane_ids(&layout, &state.pane_agents);
        let reason = if pane_regions == state.pane_regions {
            RenderDiffReason::DirtyFrame
        } else {
            // Scrollback can move the viewport without changing the visible pixels. Send an empty diff in that case so
            // clients can complete scroll-dependent state after the matching PaneRegions event.
            RenderDiffReason::RegionChanged
        };
        let update = state.render_composer.render_diff(
            &layout,
            &runtimes,
            &state.terminal_size,
            &attention_panes,
            reason,
            crate::keyboard_input::border_render_mode(state.input_mode),
        )?;
        drop(runtimes);
        drop(layout);
        (pane_regions, update)
    };
    if !self::send_pane_regions_and_render(event_writer, state, pane_regions, render_update).await? {
        return Ok(false);
    }
    *render_dirty = false;
    Ok(true)
}

fn refresh_detected_agents_if_due(state: &mut AttachedSessionState<'_>, now: Instant) -> rootcause::Result<()> {
    if let Some(detected_agents) = state.agent_detection_worker.take_finished() {
        state.detected_agents = detected_agents;
        state.detected_agents_refreshed_at = Some(now);
    }

    if state.agent_detection_worker.has_pending()
        || !crate::pane_agent::detected_agents_refresh_due(state.detected_agents_refreshed_at, now)
    {
        return Ok(());
    }

    // sysinfo refreshes the full process list and can block; the attention tick only submits work and consumes
    // completed scans so agent attention has one timing model.
    let shell_processes = {
        let runtimes = crate::server::lock_mutex(state.runtimes, "pane runtimes")?;
        runtimes.shell_processes()?
    };
    state.agent_detection_worker.request(shell_processes)
}

fn runtime_pane_metadata(state: &AttachedSessionState<'_>) -> rootcause::Result<RuntimePaneMetadata> {
    let terminal_titles = {
        let runtimes = crate::server::lock_mutex(state.runtimes, "pane runtimes")?;
        runtimes.terminal_titles()?
    };
    let runtime_cmd_labels = crate::pane_agent::runtime_cmd_labels(&state.detected_agents);
    Ok(RuntimePaneMetadata {
        runtime_cmd_labels,
        terminal_titles,
    })
}

async fn flush_cmd_label_layout(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    title_changes: Vec<(PaneId, Option<String>)>,
) -> rootcause::Result<bool> {
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let changes = {
        let mut layout = crate::server::lock_mutex(state.layout, "layout")?;
        let mut last_layout_snapshot = state.last_layout_snapshot.clone();
        let mut layout_changed = false;
        let runtime_agent_states = state.pane_agents.snapshot_states();
        let mut changes = Vec::new();
        for (pane_id, title) in title_changes {
            layout_changed |= layout.sync_terminal_titles(&[(pane_id, title.clone())]);
            let terminal_titles =
                self::terminal_titles_with_override(&runtime_metadata.terminal_titles, pane_id, title);
            let layout_snapshot = layout.snapshot_with_runtime_metadata(
                &terminal_titles,
                &runtime_metadata.runtime_cmd_labels,
                &runtime_agent_states,
            )?;
            if layout_snapshot == last_layout_snapshot {
                continue;
            }
            last_layout_snapshot = layout_snapshot.clone();
            changes.push(layout_snapshot);
        }
        if layout_changed {
            crate::state::persisted::write_metadata(&state.config.paths, &layout)?;
        }
        drop(layout);
        changes
    };

    for layout_snapshot in changes {
        // Terminal-title changes affect only sidebar metadata; avoid rebuilding the pane frame for command/cwd churn.
        if !self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn flush_pane_attention(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let now = Instant::now();
    self::refresh_detected_agents_if_due(state, now)?;
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let visible_activity_panes = state.pending_visible_activity_panes.iter().copied().collect::<Vec<_>>();
    state.pending_visible_activity_panes.clear();
    let layout_snapshot = {
        let layout = crate::server::lock_mutex(state.layout, "layout")?;
        if !state
            .pane_agents
            .sync_attention(&layout, &state.detected_agents, &visible_activity_panes, now)?
        {
            return Ok(true);
        }
        *render_dirty = true;
        let runtime_agent_states = state.pane_agents.snapshot_states();
        layout.snapshot_with_runtime_metadata(
            &runtime_metadata.terminal_titles,
            &runtime_metadata.runtime_cmd_labels,
            &runtime_agent_states,
        )?
    };

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

fn terminal_titles_with_override(
    terminal_titles: &[(PaneId, Option<String>)],
    pane_id: PaneId,
    title: Option<String>,
) -> Vec<(PaneId, Option<String>)> {
    let mut out = Vec::with_capacity(terminal_titles.len().saturating_add(1));
    let mut replaced = false;
    for (existing_pane_id, existing_title) in terminal_titles {
        if *existing_pane_id == pane_id {
            out.push((*existing_pane_id, title.clone()));
            replaced = true;
        } else {
            out.push((*existing_pane_id, existing_title.clone()));
        }
    }
    if !replaced {
        out.push((pane_id, title));
    }
    out
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
    let runtime_cmd_labels = crate::pane_agent::runtime_cmd_labels(&state.detected_agents);
    let (layout_snapshot, pane_regions, render_update) = {
        let mut layout = crate::server::lock_mutex(state.layout, "layout")?;
        let runtimes = crate::server::lock_mutex(state.runtimes, "pane runtimes")?;
        let runtime_agent_states = state.pane_agents.snapshot_states();
        let layout_snapshot = self::layout_snapshot_and_persist(
            &state.config.paths,
            &mut layout,
            &runtimes,
            &runtime_cmd_labels,
            &runtime_agent_states,
        )?;
        let pane_regions = self::pane_regions_snapshot(&layout, &runtimes, &state.terminal_size)?;
        let attention_panes = self::attention_pane_ids(&layout, &state.pane_agents);
        let render_update = state.render_composer.render_baseline(
            &layout,
            &runtimes,
            &state.terminal_size,
            &attention_panes,
            crate::keyboard_input::border_render_mode(state.input_mode),
        )?;
        drop(runtimes);
        drop(layout);
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
    match self::reap_exited_panes(&state.config.paths, state.layout, state.runtimes)? {
        ReapResult::Final => Ok(true),
        ReapResult::NoExitedPanes => Ok(false),
        ReapResult::Removed => {
            let live_panes = {
                let runtimes = crate::server::lock_mutex(state.runtimes, "pane runtimes")?;
                runtimes.pane_ids()
            };
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

async fn handle_attached_request(
    request: Option<ClientRequest>,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    heartbeat_started_at: &mut Option<tokio::time::Instant>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match request {
        Some(ClientRequest::Detach) => {
            let _sent = self::send_writer_event_with_timeout(
                event_writer,
                &ServerEvent::Detached,
                state.config.client_write_timeout,
            )
            .await?;
            Ok(false)
        }
        Some(ClientRequest::DeleteSession) => {
            crate::sessions_delete::handle_attached_delete(
                event_writer,
                state.delete_sessions,
                state.config.client_write_timeout,
            )
            .await?;
            Ok(false)
        }
        Some(ClientRequest::Input(bytes)) => {
            if !bytes.is_empty() {
                *render_dirty |= self::write_active_pane_user_input(
                    state,
                    crate::keyboard_input::input_interaction(&bytes),
                    |handle| handle.write_input(&bytes),
                )?;
            }
            Ok(true)
        }
        Some(ClientRequest::Paste(bytes)) => {
            if !bytes.is_empty() {
                // Bracketed paste can contain newlines as data; only raw input newlines mean prompt submission.
                *render_dirty |= self::write_active_pane_user_input(state, PaneUserInteraction::MayEcho, |handle| {
                    handle.write_paste(&bytes)
                })?;
            }
            Ok(true)
        }
        Some(ClientRequest::Key(key)) => self::handle_key_request(key, event_writer, state, render_dirty).await,
        Some(ClientRequest::Mouse(event)) => {
            self::handle_mouse_event_request(event, event_writer, state, render_dirty).await
        }
        Some(ClientRequest::ScrollPaneLineAt { position, direction }) => {
            let event = self::scroll_pane_line_event(position, direction, state, render_dirty)?;
            self::send_writer_event_with_timeout(event_writer, &event, state.config.client_write_timeout).await
        }
        Some(ClientRequest::FocusPaneAt(position)) => {
            self::handle_focus_pane_at_request(position, event_writer, state).await
        }
        Some(ClientRequest::FocusTab(tab_id)) => self::focus_tab_and_render(tab_id, event_writer, state).await,
        Some(ClientRequest::Resize(size)) => {
            state.terminal_size = size;
            if !self::resize_panes_and_render(event_writer, state).await? {
                return Ok(false);
            }
            Ok(true)
        }
        Some(ClientRequest::RenderResync) => {
            if !self::send_layout_and_baseline(event_writer, state).await? {
                return Ok(false);
            }
            Ok(true)
        }
        Some(ClientRequest::Ping) => {
            self::send_writer_event_with_timeout(event_writer, &ServerEvent::Pong, state.config.client_write_timeout)
                .await
        }
        Some(ClientRequest::Pong) => {
            *heartbeat_started_at = None;
            Ok(true)
        }
        Some(request @ ClientRequest::Attach(_)) => {
            let _sent = self::send_writer_event_with_timeout(
                event_writer,
                &ServerEvent::Error(ServerError::unexpected_request(request)),
                state.config.client_write_timeout,
            )
            .await?;
            Ok(false)
        }
        None => Ok(false),
    }
}

fn scroll_pane_line_event(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    state: &AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<ServerEvent> {
    let scrolled = crate::pane_scroll::handle_scroll_pane_line_at_request(
        position,
        direction,
        state.layout,
        state.runtimes,
        &state.terminal_size,
    )?;
    if scrolled {
        // Edge-drag autoscroll can outpace render IO; keep viewport changes coalesced on the render tick.
        *render_dirty = true;
    }
    // Clients keep one edge-scroll request pending until either a moved viewport renders or this no-op result arrives.
    Ok(ServerEvent::ScrollPaneLineResult {
        position,
        direction,
        scrolled,
    })
}

async fn handle_focus_pane_at_request(
    position: ClientMousePosition,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    if !crate::pane_focus::handle_focus_pane_at_request(position, state.config, state.layout, &state.terminal_size)? {
        return Ok(true);
    }
    let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
    self::send_layout_and_baseline(event_writer, state).await
}

async fn focus_tab_and_render(
    tab_id: TabId,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    if !crate::tab_focus::handle_focus_tab_request(tab_id, state.config, state.layout)? {
        return Ok(true);
    }
    let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
    self::send_layout_and_baseline(event_writer, state).await
}

async fn handle_key_request(
    key: ClientKey,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match crate::keyboard_input::resolve_key(&mut state.input_mode, &key) {
        KeyResolution::Cmd(cmd) => self::handle_cmd_request(cmd, event_writer, state).await,
        KeyResolution::Raw => {
            if !key.raw_bytes.is_empty() {
                *render_dirty |= self::write_active_pane_user_input(
                    state,
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
                ClosePaneOutcome::Final { .. } => {
                    let _sent = self::send_writer_event_with_timeout(
                        event_writer,
                        &ServerEvent::Detached,
                        state.config.client_write_timeout,
                    )
                    .await?;
                    Ok(false)
                }
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
            if !crate::pane_focus::handle_focus_pane_cmd(direction, state.config, state.layout, &state.terminal_size)? {
                return Ok(true);
            }
            let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
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
        }
        TabCmd::FocusPrevious => {
            crate::tab_focus::handle_focus_previous_tab_cmd(state.config, state.layout)?;
            let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
        }
        TabCmd::FocusNext => {
            crate::tab_focus::handle_focus_next_tab_cmd(state.config, state.layout)?;
            let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
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
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<(PaneId, PtyHandle)> {
    let active_pane = {
        let layout = crate::server::lock_mutex(layout, "layout")?;
        layout.active_pane_id()?
    };
    let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
    let handle = runtimes.handle(active_pane)?;
    drop(runtimes);
    Ok((active_pane, handle))
}

fn write_active_pane_user_input(
    state: &mut AttachedSessionState<'_>,
    interaction: PaneUserInteraction,
    write: impl FnOnce(&PtyHandle) -> rootcause::Result<bool>,
) -> rootcause::Result<bool> {
    let (pane_id, handle) = self::active_pane_handle_with_id(state.layout, state.runtimes)?;
    let render_dirty = write(&handle)?;
    state
        .pane_agents
        .record_user_interaction(pane_id, interaction, Instant::now());
    Ok(render_dirty)
}

fn acknowledge_active_agent_attention(state: &mut AttachedSessionState<'_>) -> rootcause::Result<bool> {
    let active_pane = {
        let layout = crate::server::lock_mutex(state.layout, "layout")?;
        layout.active_pane_id()?
    };
    Ok(state.pane_agents.acknowledge_attention(active_pane))
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
    let handle = {
        let runtimes = crate::server::lock_mutex(state.runtimes, "pane runtimes")?;
        let handle = runtimes.handle(*region.id())?;
        drop(runtimes);
        handle
    };
    let action = crate::pane_mouse::resolve_pane_mouse_action(event, handle.application_mode()?);
    match action {
        crate::pane_mouse::PaneMouseAction::ForwardToPty { focus, protocol } => {
            if let Some(scrolled_to_bottom) = handle.write_mouse_event(event, &region, protocol)? {
                *render_dirty |= scrolled_to_bottom;
                state
                    .pane_agents
                    .record_user_interaction(*region.id(), PaneUserInteraction::MayEcho, Instant::now());
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
            state
                .pane_agents
                .record_user_interaction(*region.id(), PaneUserInteraction::MayEcho, Instant::now());
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
    if !crate::pane_focus::handle_focus_pane_at_request(
        event.position,
        state.config,
        state.layout,
        &state.terminal_size,
    )? {
        return Ok(true);
    }
    let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
    self::send_layout_and_baseline(event_writer, state).await
}

/// Send one event on a pre-attach connection with the server's bounded write timeout.
///
/// # Errors
/// This function currently returns `Ok(false)` for send failures and timeouts instead of an error.
async fn send_connection_event_with_timeout(
    connection: &mut ServerConnection,
    event: &ServerEvent,
    client_write_timeout: Duration,
) -> rootcause::Result<bool> {
    match tokio::time::timeout(client_write_timeout, connection.send_event(event)).await {
        Ok(Ok(())) => Ok(true),
        Ok(Err(_)) | Err(_) => Ok(false),
    }
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

async fn join_client_task(handle: tokio::task::JoinHandle<rootcause::Result<()>>) -> rootcause::Result<()> {
    handle
        .await
        .unwrap_or_else(|error| Err(report!("muxr server client task panicked").attach(format!("{error}"))))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use muxr_core::SessionName;
    use muxr_core::SessionPaths;

    use super::*;
    use crate::pane_runtime::test_helpers as pane_runtime_test_helpers;
    use crate::state::SessionMetadata;

    #[test]
    fn test_layout_snapshot_and_persist_when_runtime_cmd_exists_sets_snapshot_cmd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&session, self::metadata("zsh", 1))?;
        let runtimes = pane_runtime_test_helpers::empty_runtimes();
        let pane_id = PaneId::new(1)?;

        let snapshot = self::layout_snapshot_and_persist(
            &paths,
            &mut layout,
            &runtimes,
            &[(pane_id, Some("cx".to_owned()))],
            &[],
        )?;

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
