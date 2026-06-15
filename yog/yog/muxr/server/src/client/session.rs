use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use muxr_core::AttachRequest;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::ServerEvent;
use muxr_core::TerminalSize;
use muxr_transport::ServerConnection;
use muxr_transport::ServerEventWriter;
use muxr_transport::ServerRequestReader;
use rootcause::report;

use crate::client::timers::ClientTimers;
use crate::keyboard_input::ServerInputMode;
use crate::pane::fullscreen::PaneFullscreen;
use crate::pane::render::RenderComposer;
use crate::pane::runtime::PaneRuntimes;
use crate::pane::tracked_process::PaneTrackedProcesses;
use crate::pty::PtyEvent;
use crate::pty::PtySinkGuard;
use crate::scrollback_editor::ScrollbackEditorState;
use crate::server::ServerConfig;
use crate::session::delete::DeleteSessions;
use crate::session::runtime::PANE_OUTPUT_EVENT_CHANNEL_LIMIT;
use crate::session::runtime::ReapResult;
use crate::session::runtime::SessionClientMessage;
use crate::session::runtime::SessionPaneOutputMessage;
use crate::session::runtime::SessionRuntimeTimerMessage;
use crate::state::SessionLayout;

struct ClientPtySink {
    guard: PtySinkGuard,
    pane_id: PaneId,
}

pub struct ClientSessionState<'a> {
    pub pane_tracked_processes: PaneTrackedProcesses,
    pub config: &'a ServerConfig,
    pub delete_sessions: &'a DeleteSessions,
    pub input_mode: ServerInputMode,
    pub last_layout_snapshot: LayoutSnapshot,
    pub layout: &'a mut SessionLayout,
    pub pane_fullscreen: PaneFullscreen,
    pub pane_regions: PaneRegionsSnapshot,
    pty_event_sender: &'a mpsc::SyncSender<PtyEvent>,
    pub render_composer: &'a mut RenderComposer,
    pub runtimes: &'a mut PaneRuntimes,
    pub scrollback_editor: Option<ScrollbackEditorState>,
    sink_guards: &'a mut Vec<ClientPtySink>,
    pub terminal_size: TerminalSize,
}

fn attach_pane_sinks(
    runtimes: &PaneRuntimes,
    sender: &mpsc::SyncSender<PtyEvent>,
) -> rootcause::Result<Vec<ClientPtySink>> {
    Ok(runtimes
        .attach_sinks(sender)?
        .into_iter()
        .map(|(pane_id, guard)| ClientPtySink { guard, pane_id })
        .collect())
}

fn attach_pane_sink(
    runtimes: &PaneRuntimes,
    sender: &mpsc::SyncSender<PtyEvent>,
    pane_id: PaneId,
) -> rootcause::Result<ClientPtySink> {
    Ok(ClientPtySink {
        guard: runtimes.handle(pane_id)?.attach_sink(sender.clone())?,
        pane_id,
    })
}

pub fn attach_pane_sink_to_state(state: &mut ClientSessionState<'_>, pane_id: PaneId) -> rootcause::Result<()> {
    state
        .sink_guards
        .push(self::attach_pane_sink(state.runtimes, state.pty_event_sender, pane_id)?);
    Ok(())
}

pub fn remove_pane_sink(state: &mut ClientSessionState<'_>, pane_id: PaneId) {
    state.sink_guards.retain(|sink| sink.pane_id != pane_id);
}

pub async fn handle_client(
    config: &ServerConfig,
    connection: ServerConnection,
    attach_request: AttachRequest,
    delete_sessions: &DeleteSessions,
    layout: &mut SessionLayout,
    runtimes: &mut PaneRuntimes,
) -> rootcause::Result<()> {
    crate::screen_render::resize_panes_to_layout(layout, runtimes, &attach_request.terminal_size)?;
    let (pty_event_sender, pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
    let mut sink_guards = self::attach_pane_sinks(runtimes, &pty_event_sender)?;
    let (mut request_reader, mut event_writer) = connection.split();
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    pane_tracked_processes.observe_all_runtime_pane_cmds(
        config.user_config.as_ref(),
        layout,
        runtimes,
        Instant::now(),
    )?;
    let (layout_snapshot, pane_regions, mut render_composer, render_baseline) =
        crate::screen_render::initial_client_render(
            config,
            layout,
            runtimes,
            &pane_tracked_processes,
            &attach_request.terminal_size,
        )?;
    let last_layout_snapshot = layout_snapshot.clone();
    let initial_pane_regions = pane_regions.clone();
    if !crate::screen_render::send_attach_response_and_baseline(
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
    let mut client_state = ClientSessionState {
        pane_tracked_processes,
        config,
        delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot,
        layout,
        pane_fullscreen: PaneFullscreen::default(),
        pane_regions: initial_pane_regions,
        pty_event_sender: &pty_event_sender,
        render_composer: &mut render_composer,
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
        self::remove_pane_sink(&mut client_state, editor_pane_id);
    }
    drop(client_state);

    drop(sink_guards);
    drop(pty_event_sender);
    drop(async_pty_receiver);
    bridge_handle
        .await
        .map_err(|error| report!("muxr server pty bridge task panicked").attach(format!("{error}")))?;
    match result {
        Ok(()) => restore_result.map(|_| ()),
        Err(error) => {
            let _ = restore_result.inspect_err(|restore_error| {
                crate::session::tracing::scrollback::restore_failed(restore_error);
            });
            Err(error)
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the two biased select branches keep request/output priority ordering explicit"
)]
async fn run_client_session(
    request_reader: &mut ServerRequestReader,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<SessionPaneOutputMessage>,
) -> rootcause::Result<()> {
    let mut timers = ClientTimers::new(state.config)?;
    timers.sync_tracked_process_quiet_deadline(state.pane_tracked_processes.next_quiet_deadline()?)?;
    let mut heartbeat_started_at: Option<tokio::time::Instant> = None;
    let mut render_dirty = false;
    let mut request_turn = false;

    loop {
        if crate::client::lifecycle::client_should_exit(
            state.sink_guards.iter().map(|sink| sink.guard.is_output_current()),
            state.config.client_heartbeat_timeout,
            state.delete_sessions,
            heartbeat_started_at,
        ) {
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
                    let message = SessionClientMessage::from_request(request?);
                    if !crate::request_router::handle_client_message(message, event_writer, state, &mut timers, &mut heartbeat_started_at, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                event = pty_event_receiver.recv() => {
                    request_turn = true;
                    if !crate::pty_output::handle_pane_output_message(event, event_writer, state, &mut timers, &mut render_dirty).await? {
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
                    if !crate::pty_output::handle_pane_output_message(event, event_writer, state, &mut timers, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                request = request_reader.recv_request() => {
                    request_turn = false;
                    let message = SessionClientMessage::from_request(request?);
                    if !crate::request_router::handle_client_message(message, event_writer, state, &mut timers, &mut heartbeat_started_at, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
            }
        }
    }
}

async fn handle_session_runtime_timer_message(
    message: SessionRuntimeTimerMessage,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    heartbeat_started_at: &mut Option<tokio::time::Instant>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match message {
        SessionRuntimeTimerMessage::HeartbeatTick => {
            self::send_heartbeat_if_idle(event_writer, state.config.client_write_timeout, heartbeat_started_at).await
        }
        SessionRuntimeTimerMessage::RenderDeadlineReached => {
            let keep_attached = crate::screen_render::flush_render_diff(event_writer, state, render_dirty).await?;
            // `Sleep` stays ready after it fires. Complete the frame immediately so the one-shot wakeup is disabled
            // and the next dirty frame is rate-limited from this render attempt.
            timers.complete_render_frame()?;
            Ok(keep_attached)
        }
        SessionRuntimeTimerMessage::CmdHandoffSampleReady => {
            crate::screen_render::handle_cmd_handoff_sample(timers, event_writer, state, render_dirty).await
        }
        SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached => {
            timers.disable_tracked_process_quiet_sleep()?;
            if !crate::screen_render::flush_pane_attention(timers, event_writer, state, render_dirty).await? {
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

    if !crate::event_writer::send_event_with_timeout(event_writer, &ServerEvent::Ping, client_write_timeout).await? {
        return Ok(false);
    }
    *heartbeat_started_at = Some(tokio::time::Instant::now());
    Ok(true)
}

pub async fn handle_reaped_panes(
    state: &mut ClientSessionState<'_>,
    event_writer: &mut ServerEventWriter,
) -> rootcause::Result<bool> {
    let previous_pane_before_restore = state.layout.active_pane_id()?;
    let restored_editor = crate::scrollback_editor::restore_before_reap_if_needed(state)?;
    if let Some(editor_pane_id) = restored_editor.editor_pane_id {
        self::remove_pane_sink(state, editor_pane_id);
    }
    let previous_pane_before_reap = state.layout.active_pane_id()?;
    match crate::session::runtime::reap_exited_panes(state.config, state.layout, state.runtimes)? {
        ReapResult::Final => Ok(true),
        ReapResult::NoExitedPanes => {
            if !restored_editor.restored() {
                return Ok(false);
            }
            crate::pane::focus::write_active_pane_focus_events(previous_pane_before_restore, state)?;
            Ok(!crate::screen_render::send_layout_and_baseline(event_writer, state).await?)
        }
        ReapResult::Removed => {
            let live_panes = state.runtimes.pane_ids();
            state.sink_guards.retain(|sink| live_panes.contains(&sink.pane_id));
            let previous_pane = if restored_editor.restored() {
                previous_pane_before_restore
            } else {
                previous_pane_before_reap
            };
            crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
            Ok(!crate::screen_render::resize_panes_and_render(event_writer, state).await?)
        }
    }
}
