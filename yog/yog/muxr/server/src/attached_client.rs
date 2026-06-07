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

use crate::attached_client_timers::AttachedClientTimers;
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
use crate::pane_tracked_process::TrackedProcessUserInteraction;
use crate::pty::PtyEvent;
use crate::pty::PtyHandle;
use crate::pty::PtySinkGuard;
use crate::server::ServerConfig;
use crate::sessions_delete::DeleteSessions;
use crate::state::SessionLayout;

const CLIENT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
const OUTPUT_EVENT_CHANNEL_LIMIT: usize = 1024;

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
    pane_tracked_processes: PaneTrackedProcesses,
    config: &'a ServerConfig,
    delete_sessions: &'a DeleteSessions,
    input_mode: ServerInputMode,
    last_layout_snapshot: LayoutSnapshot,
    layout: &'a Mutex<SessionLayout>,
    pane_regions: PaneRegionsSnapshot,
    pty_event_sender: &'a mpsc::SyncSender<PtyEvent>,
    render_composer: &'a mut RenderComposer,
    runtimes: &'a Mutex<PaneRuntimes>,
    sink_guards: &'a mut Vec<AttachedPtySink>,
    terminal_size: TerminalSize,
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
    config: &ServerConfig,
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
        crate::state::persisted::write_metadata(&config.paths, &layout)?;
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
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    pane_tracked_processes: &PaneTrackedProcesses,
    terminal_size: &TerminalSize,
) -> rootcause::Result<(LayoutSnapshot, PaneRegionsSnapshot, RenderComposer, RenderUpdate)> {
    let mut render_composer = RenderComposer::default();
    let mut layout = crate::server::lock_mutex(layout, "layout")?;
    let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
    let tracked_processes = pane_tracked_processes.snapshot();
    let layout_snapshot = self::layout_snapshot_and_persist(&config.paths, &mut layout, &runtimes, &tracked_processes)?;
    let pane_regions = self::pane_regions_snapshot(&layout, &runtimes, terminal_size)?;
    let attention_panes = self::attention_pane_ids(&layout, pane_tracked_processes);
    let render_baseline = render_composer.render_baseline(
        PaneRenderConfig {
            border_styles: config.user_config.pane_borders,
            mode: crate::pane_borders::BorderRenderMode::Focus,
            pane_attention: config.user_config.pane_attention,
            pane_dim: config.user_config.pane_dim,
        },
        &layout,
        &runtimes,
        terminal_size,
        &attention_panes,
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
    tracked_processes: &PaneTrackedProcessSnapshot,
) -> rootcause::Result<LayoutSnapshot> {
    let synced = runtimes.sync_layout_terminal_titles(layout)?;
    if synced.layout_changed() {
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
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    pane_tracked_processes.observe_all_runtime_pane_cmds(
        config.user_config.as_ref(),
        layout,
        runtimes,
        Instant::now(),
    )?;
    let (layout_snapshot, pane_regions, mut render_composer, render_baseline) = self::initial_attached_render(
        config,
        layout,
        runtimes,
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

    let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(OUTPUT_EVENT_CHANNEL_LIMIT);
    let bridge_handle = tokio::task::spawn_blocking(move || {
        while let Ok(event) = pty_event_receiver.recv() {
            if async_pty_sender.blocking_send(event).is_err() {
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
        layout,
        pane_regions: attached_pane_regions,
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
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<PtyEvent>,
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
                () = timers.cmd_handoff_sample.as_mut() => {
                    if !self::handle_cmd_handoff_sample(&mut timers, event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                () = timers.tracked_process_quiet_sleep.as_mut() => {
                    timers.disable_tracked_process_quiet_sleep()?;
                    if !self::flush_pane_attention(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                    timers.sync_tracked_process_quiet_deadline(state.pane_tracked_processes.next_quiet_deadline()?)?;
                },
                request = request_reader.recv_request() => {
                    request_turn = false;
                    if !self::handle_attached_request(request?, event_writer, state, &mut timers, &mut heartbeat_started_at, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                event = pty_event_receiver.recv() => {
                    request_turn = true;
                    if !self::handle_pty_event(event, event_writer, state, &mut timers, &mut render_dirty).await? {
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
                () = timers.cmd_handoff_sample.as_mut() => {
                    if !self::handle_cmd_handoff_sample(&mut timers, event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                () = timers.tracked_process_quiet_sleep.as_mut() => {
                    timers.disable_tracked_process_quiet_sleep()?;
                    if !self::flush_pane_attention(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                    timers.sync_tracked_process_quiet_deadline(state.pane_tracked_processes.next_quiet_deadline()?)?;
                },
                event = pty_event_receiver.recv() => {
                    // Output gets one turn, then client requests get first chance so detach/pong cannot starve.
                    request_turn = true;
                    if !self::handle_pty_event(event, event_writer, state, &mut timers, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                request = request_reader.recv_request() => {
                    request_turn = false;
                    if !self::handle_attached_request(request?, event_writer, state, &mut timers, &mut heartbeat_started_at, &mut render_dirty).await? {
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
    timers: &mut AttachedClientTimers,
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
        let layout = crate::server::lock_mutex(state.layout, "layout")?;
        let runtimes = crate::server::lock_mutex(state.runtimes, "pane runtimes")?;
        let pane_regions = self::pane_regions_snapshot(&layout, &runtimes, &state.terminal_size)?;
        let attention_panes = self::attention_pane_ids(&layout, &state.pane_tracked_processes);
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
            &layout,
            &runtimes,
            &state.terminal_size,
            &attention_panes,
            reason,
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

fn runtime_pane_metadata(state: &AttachedSessionState<'_>) -> rootcause::Result<PaneRuntimeMetadata> {
    let (terminal_titles, startup_cmd_labels) = {
        let runtimes = crate::server::lock_mutex(state.runtimes, "pane runtimes")?;
        (runtimes.terminal_titles()?, runtimes.startup_cmd_labels())
    };
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
        let mut layout = crate::server::lock_mutex(state.layout, "layout")?;
        let mut last_layout_snapshot = state.last_layout_snapshot.clone();
        let mut layout_changed = false;
        let mut changes = Vec::new();
        for (pane_id, title) in title_changes {
            layout_changed |= layout.sync_terminal_titles(&[(pane_id, title.clone())]);
            let runtime_metadata = runtime_metadata.with_terminal_title_override(pane_id, title);
            let layout_snapshot = layout.snapshot_with_runtime_metadata(&runtime_metadata.pane_snapshot_fields())?;
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
    let layout_snapshot = {
        let layout = crate::server::lock_mutex(state.layout, "layout")?;
        layout.snapshot_with_runtime_metadata(&runtime_metadata.pane_snapshot_fields())?
    };
    *render_dirty = true;
    self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await
}

async fn flush_pane_attention(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let now = Instant::now();
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let layout_snapshot = {
        let layout = crate::server::lock_mutex(state.layout, "layout")?;
        if !state.pane_tracked_processes.mark_quiet_deadlines(&layout, now)? {
            return Ok(true);
        }
        *render_dirty = true;
        layout.snapshot_with_runtime_metadata(&runtime_metadata.pane_snapshot_fields())?
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
        let mut layout = crate::server::lock_mutex(state.layout, "layout")?;
        let runtimes = crate::server::lock_mutex(state.runtimes, "pane runtimes")?;
        let tracked_processes = state.pane_tracked_processes.snapshot();
        let layout_snapshot =
            self::layout_snapshot_and_persist(&state.config.paths, &mut layout, &runtimes, &tracked_processes)?;
        let pane_regions = self::pane_regions_snapshot(&layout, &runtimes, &state.terminal_size)?;
        let attention_panes = self::attention_pane_ids(&layout, &state.pane_tracked_processes);
        let render_update = state.render_composer.render_baseline(
            PaneRenderConfig {
                border_styles: state.config.user_config.pane_borders,
                mode: crate::keyboard_input::border_render_mode(state.input_mode),
                pane_attention: state.config.user_config.pane_attention,
                pane_dim: state.config.user_config.pane_dim,
            },
            &layout,
            &runtimes,
            &state.terminal_size,
            &attention_panes,
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
    match self::reap_exited_panes(state.config, state.layout, state.runtimes)? {
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
    timers: &mut AttachedClientTimers,
    heartbeat_started_at: &mut Option<tokio::time::Instant>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let Some(request) = request else {
        return Ok(false);
    };

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

    use muxr_config::MuxrConfig;
    use muxr_core::SessionName;
    use muxr_core::SessionPaths;

    use super::*;
    use crate::pane_cmd::PaneCmd;
    use crate::pane_cmd::PaneCmdObservation;
    use crate::pane_runtime::test_helpers as pane_runtime_test_helpers;
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
