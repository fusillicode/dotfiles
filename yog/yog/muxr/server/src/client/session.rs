use std::collections::BTreeSet;
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
use tokio::sync::mpsc::error::TryRecvError;

use super::quiet::QuietTurn;
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

// A quiet-boundary batch coalesces many PTY wakeup markers into one handler call, but stays small enough to yield back
// to the request/output select loop before quiet clearing if the channel is full.
const QUIET_OUTPUT_DRAIN_BATCH_LIMIT: usize = 32;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReapedPanes {
    Unchanged,
    LayoutChanged,
    Stop,
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

fn remove_pane_tracked_process(state: &mut ClientSessionState<'_>, pane_id: PaneId) {
    // Pane IDs are allocated from the live layout max, so a removed high ID can be reused before the next quiet sweep.
    state.pane_tracked_processes.remove_pane(pane_id);
}

fn remove_pane_timer(timers: &mut ClientTimers, pane_id: PaneId) -> rootcause::Result<()> {
    // Prompt-submit sampling fires after a short delay, so pane removal must clear the timer entry before the runtime
    // disappears; otherwise the later sample can ask for a stale pane handle and tear down the client session.
    timers.remove_cmd_handoff_sample_pane(pane_id)
}

fn remove_pane_client_resources(state: &mut ClientSessionState<'_>, pane_id: PaneId) {
    // This cleanup is used during attach/session teardown paths without live client timers. Live pane removal must go
    // through `remove_pane_from_client_state` so delayed command-handoff samples are also cleared.
    state.sink_guards.retain(|sink| sink.pane_id != pane_id);
    self::remove_pane_tracked_process(state, pane_id);
}

fn remove_pane_tracking_state(
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    pane_id: PaneId,
) -> rootcause::Result<()> {
    self::remove_pane_tracked_process(state, pane_id);
    self::remove_pane_timer(timers, pane_id)
}

pub fn remove_pane_from_client_state(
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    pane_id: PaneId,
) -> rootcause::Result<()> {
    self::remove_pane_client_resources(state, pane_id);
    self::remove_pane_timer(timers, pane_id)
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
        self::remove_pane_client_resources(&mut client_state, editor_pane_id);
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

async fn run_client_session(
    request_reader: &mut ServerRequestReader,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<SessionPaneOutputMessage>,
) -> rootcause::Result<()> {
    self::run_client_session_loop(request_reader, event_writer, state, pty_event_receiver, false).await
}

#[cfg(test)]
async fn run_client_session_after_output_turn(
    request_reader: &mut ServerRequestReader,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<SessionPaneOutputMessage>,
) -> rootcause::Result<()> {
    // An output turn flips the loop to request-priority before the next select. Seeding that state lets tests cover
    // request-deferred quiet handling without racing a real timer against a client request.
    self::run_client_session_loop(request_reader, event_writer, state, pty_event_receiver, true).await
}

#[expect(
    clippy::too_many_lines,
    reason = "the two biased select branches keep request/output priority ordering explicit"
)]
async fn run_client_session_loop(
    request_reader: &mut ServerRequestReader,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<SessionPaneOutputMessage>,
    mut request_turn: bool,
) -> rootcause::Result<()> {
    let mut timers = ClientTimers::new(state.config)?;
    timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
    let mut heartbeat_started_at: Option<tokio::time::Instant> = None;
    let mut render_dirty = false;
    let mut quiet_turn = QuietTurn::default();

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
        let ready_quiet = quiet_turn.take_ready(timers.tracked_process_quiet_sleep_deadline_has_passed());
        let mut skip_quiet_this_turn = false;
        if ready_quiet.drains_before_clear() {
            match self::drain_queued_output_before_quiet(
                pty_event_receiver,
                event_writer,
                state,
                &mut timers,
                &mut render_dirty,
            )
            .await?
            {
                OutputDrain::NoOutput => {}
                OutputDrain::Output => request_turn = true,
                OutputDrain::BatchLimitReached => {
                    request_turn = true;
                    // A full drain batch means output stayed hot. Re-arm quiet so requests get a turn before Busy can
                    // clear.
                    quiet_turn.after_output(timers.tracked_process_quiet_sleep_deadline_has_passed());
                    skip_quiet_this_turn = true;
                }
                OutputDrain::Detached => return Ok(()),
            }
        }
        if !skip_quiet_this_turn
            && ready_quiet.drains_before_clear()
            && timers.tracked_process_quiet_sleep_deadline_has_passed()
        {
            if !self::handle_session_runtime_timer_message(
                SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached,
                event_writer,
                state,
                &mut timers,
                &mut heartbeat_started_at,
                &mut render_dirty,
            )
            .await?
            {
                return Ok(());
            }
            continue;
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
                request = request_reader.recv_request() => {
                    let message = SessionClientMessage::from_request(request?);
                    if !crate::request_router::handle_client_message(message, event_writer, state, &mut timers, &mut heartbeat_started_at, &mut render_dirty).await? {
                        return Ok(());
                    }
                    request_turn = false;
                    quiet_turn.after_request(timers.tracked_process_quiet_sleep_deadline_has_passed());
                },
                event = pty_event_receiver.recv() => {
                    request_turn = true;
                    if !crate::pty_output::handle_pane_output_message(event, event_writer, state, &mut timers, &mut render_dirty).await? {
                        return Ok(());
                    }
                    quiet_turn.after_output(timers.tracked_process_quiet_sleep_deadline_has_passed());
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
                event = pty_event_receiver.recv() => {
                    // Output gets one turn, then client requests get first chance so detach/pong cannot starve.
                    request_turn = true;
                    if !crate::pty_output::handle_pane_output_message(event, event_writer, state, &mut timers, &mut render_dirty).await? {
                        return Ok(());
                    }
                    quiet_turn.after_output(timers.tracked_process_quiet_sleep_deadline_has_passed());
                },
                request = request_reader.recv_request() => {
                    let message = SessionClientMessage::from_request(request?);
                    if !crate::request_router::handle_client_message(message, event_writer, state, &mut timers, &mut heartbeat_started_at, &mut render_dirty).await? {
                        return Ok(());
                    }
                    request_turn = false;
                    quiet_turn.after_request(timers.tracked_process_quiet_sleep_deadline_has_passed());
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
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    render_dirty: &mut bool,
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
                    if crate::pty_output::handle_pane_output_message(None, event_writer, state, timers, render_dirty)
                        .await?
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

    if !crate::pty_output::handle_pane_output_message(event, event_writer, state, timers, render_dirty).await? {
        return Ok(OutputDrain::Detached);
    }
    if batch_limit_reached && timers.tracked_process_quiet_sleep_deadline_has_passed() {
        return Ok(OutputDrain::BatchLimitReached);
    }
    Ok(OutputDrain::Output)
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
            timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
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
            if !restored_editor.restored() {
                return Ok(ReapedPanes::Unchanged);
            }
            crate::pane::focus::write_active_pane_focus_events(previous_pane_before_restore, state)?;
            self::acknowledge_active_tracked_process(state)?;
            if crate::screen_render::send_layout_and_baseline(event_writer, state).await? {
                Ok(ReapedPanes::LayoutChanged)
            } else {
                Ok(ReapedPanes::Stop)
            }
        }
        ReapResult::Removed { pane_ids } => {
            for pane_id in &pane_ids {
                self::remove_pane_tracking_state(state, timers, *pane_id)?;
            }
            // Reap can remove multiple panes; update per-pane tracking/timers above, then sweep client sink guards
            // against the removed ids once.
            let removed_panes = pane_ids.iter().copied().collect::<BTreeSet<_>>();
            state.sink_guards.retain(|sink| !removed_panes.contains(&sink.pane_id));
            let previous_pane = if restored_editor.restored() {
                previous_pane_before_restore
            } else {
                previous_pane_before_reap
            };
            crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
            self::acknowledge_active_tracked_process(state)?;
            if crate::screen_render::resize_panes_and_render(event_writer, state).await? {
                Ok(ReapedPanes::LayoutChanged)
            } else {
                Ok(ReapedPanes::Stop)
            }
        }
    }
}

pub fn acknowledge_active_tracked_process(state: &mut ClientSessionState<'_>) -> rootcause::Result<bool> {
    let active_pane = state.layout.active_pane_id()?;
    // Close/reap fallback focus is not a runtime sample. Only acknowledge already-known attention here; command
    // observation stays with output and explicit focus paths so a transient shell sample cannot clear unrelated work.
    Ok(state.pane_tracked_processes.acknowledge_attention(active_pane))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;
    use std::time::Instant;

    use muxr_config::ProcessMatcher;
    use muxr_config::ScrollbackEditorConfig;
    use muxr_config::TrackedProcess;
    use muxr_config::TrackedProcessId;
    use muxr_core::ClientKey;
    use muxr_core::ClientKeyCode;
    use muxr_core::ClientKeyModifiers;
    use muxr_core::ClientMouseEvent;
    use muxr_core::ClientMouseEventPhase;
    use muxr_core::ClientMousePosition;
    use muxr_core::ClientRequest;
    use muxr_core::PaneId;
    use muxr_core::ServerEvent;
    use muxr_core::TabId;
    use muxr_core::TerminalSize;
    use muxr_core::TrackedProcessState;
    use muxr_transport::ClientConnection;
    use muxr_transport::ClientEventReader;
    use muxr_transport::ServerListener;

    use super::*;
    use crate::pane::cmd::PaneCmd;
    use crate::pane::cmd::PaneCmdObservation;
    use crate::pane::cmd::PaneCmdSnapshot;
    use crate::pane::split::PaneSplitAxis;
    use crate::pane::tracked_process::TrackedProcessUserInteraction;
    use crate::pty::ShellCmd;
    use crate::session::start_seed::SessionStartSeed;
    use crate::state::SessionMetadata;
    use crate::terminal::TerminalApplicationMode;
    use crate::terminal::TerminalScreenMode;
    use crate::terminal::TerminalSnapshot;

    #[tokio::test]
    async fn test_handle_pane_output_message_when_active_pane_exits_drops_quiet_deadline_after_reap()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        let other_pane_id = PaneId::new(2)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        pane_tracked_processes.record_user_interaction(
            &layout,
            pane_id,
            TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_secs(2))?,
        )?;
        let mut timers = ClientTimers::new(&config)?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
        let focused_deadline = timers.tracked_process_quiet_sleep.deadline();

        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: Vec::new(),
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        runtimes.handle(pane_id)?.write_input(b"exit\n")?;
        self::wait_for_pane_exit(&runtimes, pane_id)?;
        let (mut event_writer, client_drain) = self::connect_client_event_drain(&config).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let mut render_dirty = false;

        let keep_attached = crate::pty_output::handle_pane_output_message(
            Some(SessionPaneOutputMessage::PaneExited),
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut render_dirty,
        )
        .await?;

        assert2::assert!(keep_attached);
        pretty_assertions::assert_eq!(state.layout.active_pane_id()?, other_pane_id);
        pretty_assertions::assert_eq!(
            state.pane_tracked_processes.attention_pane_ids(state.layout),
            Vec::new()
        );
        pretty_assertions::assert_eq!(state.pane_tracked_processes.next_quiet_deadline(state.layout)?, None);
        assert2::assert!(timers.tracked_process_quiet_sleep.deadline() > focused_deadline);
        self::abort_client_drain(client_drain).await;
        Ok(())
    }

    #[tokio::test]
    #[expect(
        clippy::too_many_lines,
        reason = "the test keeps the multi-pane reap, client-resource cleanup, and stale tracked-state assertions together"
    )]
    async fn test_handle_pane_output_message_when_batch_reap_removes_panes_drops_client_resources()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let first_exited_pane = PaneId::new(1)?;
        let second_exited_pane = PaneId::new(2)?;
        let surviving_pane = PaneId::new(3)?;
        layout.split_active_pane(
            config.user_config.layout,
            self::metadata("sh", 3),
            PaneSplitAxis::Horizontal,
        )?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let then = Instant::now();
        for pane_id in [first_exited_pane, second_exited_pane, surviving_pane] {
            pane_tracked_processes.observe_pane_cmd(
                config.user_config.as_ref(),
                pane_id,
                &self::fg_tracked_process("codex"),
                then,
            );
        }
        let mut timers = ClientTimers::new(&config)?;
        timers.schedule_cmd_handoff_sample(first_exited_pane)?;
        timers.schedule_cmd_handoff_sample(second_exited_pane)?;
        timers.schedule_cmd_handoff_sample(surviving_pane)?;

        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: Vec::new(),
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let (mut event_writer, client_drain) = self::connect_client_event_drain(&config).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let mut sink_guards = super::attach_pane_sinks(&runtimes, &pty_event_sender)?;
        runtimes.handle(first_exited_pane)?.write_input(b"exit\n")?;
        runtimes.handle(second_exited_pane)?.write_input(b"exit\n")?;
        self::wait_for_pane_exit(&runtimes, first_exited_pane)?;
        self::wait_for_pane_exit(&runtimes, second_exited_pane)?;
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let mut render_dirty = false;

        let keep_attached = crate::pty_output::handle_pane_output_message(
            Some(SessionPaneOutputMessage::PaneExited),
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut render_dirty,
        )
        .await?;

        let sink_guard_pane_ids = state.sink_guards.iter().map(|sink| sink.pane_id).collect::<Vec<_>>();
        let snapshot = state.pane_tracked_processes.snapshot(state.layout);
        let tracked_process_pane_ids = snapshot.panes().map(|(pane_id, _pane)| pane_id).collect::<Vec<_>>();
        let removed_tracked_processes = (
            state.pane_tracked_processes.remove_pane(first_exited_pane),
            state.pane_tracked_processes.remove_pane(second_exited_pane),
        );
        // Batch reap returns every removed pane; this end-to-end assertion keeps all related client resources in sync.
        pretty_assertions::assert_eq!(
            (
                keep_attached,
                state.layout.pane_ids(),
                state.runtimes.pane_ids(),
                sink_guard_pane_ids,
                tracked_process_pane_ids,
                removed_tracked_processes,
                self::tracked_process_snapshot_state(&snapshot, surviving_pane)?,
                timers.take_cmd_handoff_sample_panes()?,
            ),
            (
                true,
                vec![surviving_pane],
                vec![surviving_pane],
                vec![surviving_pane],
                vec![surviving_pane],
                (false, false),
                TrackedProcessState::Busy,
                vec![surviving_pane],
            )
        );
        self::abort_client_drain(client_drain).await;
        Ok(())
    }

    #[tokio::test]
    async fn test_handle_client_message_when_focus_pane_at_changes_active_pane_resyncs_quiet_deadline()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        config.shell_cmd = crate::server::test_helpers::shell_cmd("/bin/cat");
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        let other_pane_id = PaneId::new(2)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let target_position = self::pane_position(&layout, &terminal_size, other_pane_id)?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        pane_tracked_processes.record_user_interaction(
            &layout,
            pane_id,
            TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_secs(2))?,
        )?;
        let mut timers = ClientTimers::new(&config)?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
        let focused_deadline = timers.tracked_process_quiet_sleep.deadline();

        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: Vec::new(),
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let (mut event_writer, client_drain) = self::connect_client_event_drain(&config).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let mut heartbeat_started_at = None;
        let mut render_dirty = false;

        let keep_attached = crate::request_router::handle_client_message(
            SessionClientMessage::Request(ClientRequest::FocusPaneAt(target_position)),
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut heartbeat_started_at,
            &mut render_dirty,
        )
        .await?;

        assert2::assert!(keep_attached);
        pretty_assertions::assert_eq!(state.layout.active_pane_id()?, other_pane_id);
        assert2::assert!(timers.tracked_process_quiet_sleep.deadline() < focused_deadline);
        self::abort_client_drain(client_drain).await;
        Ok(())
    }

    #[tokio::test]
    async fn test_handle_client_message_when_close_pane_focuses_unseen_fallback_marks_seen() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        config.shell_cmd = crate::server::test_helpers::shell_cmd("/bin/cat");
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let active_pane_id = PaneId::new(1)?;
        let fallback_pane_id = PaneId::new(2)?;
        layout.active_tab_mut()?.focus_pane(active_pane_id)?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            fallback_pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        pretty_assertions::assert_eq!(
            pane_tracked_processes.mark_quiet_deadlines(&layout, self::instant_after(then, Duration::from_secs(3))?,)?,
            crate::pane::tracked_process::TrackedProcessAttention::Unseen {
                pane_ids: vec![fallback_pane_id]
            }
        );
        let mut timers = ClientTimers::new(&config)?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;

        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: Vec::new(),
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let (mut event_writer, client_drain) = self::connect_client_event_drain(&config).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let mut heartbeat_started_at = None;
        let mut render_dirty = false;

        let keep_attached = crate::request_router::handle_client_message(
            SessionClientMessage::Request(ClientRequest::Key(ClientKey {
                code: ClientKeyCode::Char('W'),
                modifiers: ClientKeyModifiers::SHIFT_ALT,
                raw_bytes: Vec::new(),
            })),
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut heartbeat_started_at,
            &mut render_dirty,
        )
        .await?;

        assert2::assert!(keep_attached);
        pretty_assertions::assert_eq!(state.layout.active_pane_id()?, fallback_pane_id);
        let snapshot = state.pane_tracked_processes.snapshot(state.layout);
        let fallback = snapshot
            .panes()
            .find(|(pane_id, _pane)| *pane_id == fallback_pane_id)
            .map(|(_pane_id, pane)| pane)
            .ok_or_else(|| rootcause::report!("expected fallback pane tracked state"))?;
        pretty_assertions::assert_eq!(fallback.state(), TrackedProcessState::Seen);
        self::abort_client_drain(client_drain).await;
        Ok(())
    }

    #[tokio::test]
    async fn test_handle_client_message_when_input_prompt_submit_marks_seen_tracked_process_busy()
    -> rootcause::Result<()> {
        self::assert_prompt_submit_marks_seen_tracked_process_busy(ClientRequest::Input(b"\r".to_vec())).await
    }

    #[tokio::test]
    async fn test_handle_client_message_when_key_prompt_submit_marks_seen_tracked_process_busy() -> rootcause::Result<()>
    {
        self::assert_prompt_submit_marks_seen_tracked_process_busy(ClientRequest::Key(ClientKey {
            code: ClientKeyCode::Enter,
            modifiers: ClientKeyModifiers::NONE,
            raw_bytes: b"\r".to_vec(),
        }))
        .await
    }

    async fn assert_prompt_submit_marks_seen_tracked_process_busy(request: ClientRequest) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let then = Instant::now();
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        pretty_assertions::assert_eq!(
            pane_tracked_processes.mark_quiet_deadlines(&layout, self::instant_after(then, Duration::from_secs(3))?,)?,
            crate::pane::tracked_process::TrackedProcessAttention::Seen
        );

        let mut timers = ClientTimers::new(&config)?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: Vec::new(),
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        pretty_assertions::assert_eq!(
            self::tracked_process_state(&layout_snapshot, pane_id)?,
            TrackedProcessState::Seen
        );
        let listener = ServerListener::bind(&config.paths.socket)?;
        let (client_connection, server_connection) =
            tokio::try_join!(ClientConnection::connect(&config.paths.socket), listener.accept())?;
        let (mut client_reader, _client_writer) = client_connection.split();
        let (_request_reader, mut event_writer) = server_connection.split();
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let mut heartbeat_started_at = None;
        let mut render_dirty = false;

        let keep_attached = crate::request_router::handle_client_message(
            SessionClientMessage::Request(request),
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut heartbeat_started_at,
            &mut render_dirty,
        )
        .await?;

        assert2::assert!(keep_attached);
        let Some(ServerEvent::SidebarLayout(layout_snapshot)) = self::recv_test_event(&mut client_reader).await? else {
            return Err(rootcause::report!(
                "expected muxr prompt submit tracked-process layout update"
            ));
        };
        pretty_assertions::assert_eq!(
            self::tracked_process_state(&layout_snapshot, pane_id)?,
            TrackedProcessState::Busy
        );
        assert2::assert!(!timers.tracked_process_quiet_sleep_deadline_has_passed());
        Ok(())
    }

    #[tokio::test]
    async fn test_handle_client_message_when_focused_input_precedes_quiet_deadline_extends_busy()
    -> rootcause::Result<()> {
        self::assert_focused_may_echo_request_precedes_quiet_deadline_extends_busy(ClientRequest::Input(b"x".to_vec()))
            .await
    }

    #[tokio::test]
    async fn test_handle_client_message_when_paste_precedes_quiet_deadline_extends_busy() -> rootcause::Result<()> {
        self::assert_focused_may_echo_request_precedes_quiet_deadline_extends_busy(ClientRequest::Paste(b"x".to_vec()))
            .await
    }

    #[tokio::test]
    async fn test_handle_client_message_when_raw_key_precedes_quiet_deadline_extends_busy() -> rootcause::Result<()> {
        self::assert_focused_may_echo_request_precedes_quiet_deadline_extends_busy(ClientRequest::Key(ClientKey {
            code: ClientKeyCode::Char('x'),
            modifiers: ClientKeyModifiers::NONE,
            raw_bytes: b"x".to_vec(),
        }))
        .await
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the helper keeps the focused-input request, quiet timer, and final state assertions in one scenario"
    )]
    async fn assert_focused_may_echo_request_precedes_quiet_deadline_extends_busy(
        request: ClientRequest,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        Arc::make_mut(&mut config.user_config)
            .tracked_processes
            .push(TrackedProcess {
                id: TrackedProcessId::Codex,
                label: "cx",
                matchers: vec![ProcessMatcher::ExactExecutable("cat")],
                quiet_threshold: Duration::from_millis(30),
            });
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: vec![(pane_id, ShellCmd::with_args("/bin/cat", Vec::<String>::new())?)],
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        self::wait_for_runtime_fg_cmd(&runtimes, pane_id, "cat")?;
        let then = Instant::now()
            .checked_sub(Duration::from_millis(60))
            .ok_or_else(|| rootcause::report!("test instant underflowed"))?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("cat"),
            then,
        );
        let mut timers = ClientTimers::new(&config)?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
        assert2::assert!(timers.tracked_process_quiet_sleep_deadline_has_passed());
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let (mut event_writer, client_drain) = self::connect_client_event_drain(&config).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let mut heartbeat_started_at = None;
        let mut render_dirty = false;

        let keep_attached = crate::request_router::handle_client_message(
            SessionClientMessage::Request(request),
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut heartbeat_started_at,
            &mut render_dirty,
        )
        .await?;

        assert2::assert!(keep_attached);
        pretty_assertions::assert_eq!(
            self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
            TrackedProcessState::Busy
        );
        assert2::assert!(!timers.tracked_process_quiet_sleep_deadline_has_passed());

        tokio::time::sleep(Duration::from_millis(45)).await;
        let keep_attached = self::handle_session_runtime_timer_message(
            SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached,
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut heartbeat_started_at,
            &mut render_dirty,
        )
        .await?;
        assert2::assert!(keep_attached);
        pretty_assertions::assert_eq!(
            self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
            TrackedProcessState::Seen
        );
        self::abort_client_drain(client_drain).await;
        Ok(())
    }

    #[tokio::test]
    async fn test_handle_client_message_when_mouse_forward_precedes_quiet_deadline_extends_busy()
    -> rootcause::Result<()> {
        self::assert_mouse_request_precedes_quiet_deadline_extends_busy(
            "printf '\\033[?1002h\\033[?1006hready\\n'; exec /bin/cat",
            |mode| {
                assert2::assert!(mode.mouse_protocol.is_some());
                Ok(())
            },
        )
        .await
    }

    #[tokio::test]
    async fn test_handle_client_message_when_faux_scroll_precedes_quiet_deadline_extends_busy() -> rootcause::Result<()>
    {
        self::assert_mouse_request_precedes_quiet_deadline_extends_busy(
            "printf '\\033[?1049hready\\n'; exec /bin/cat",
            |mode| {
                pretty_assertions::assert_eq!(mode.screen_mode, TerminalScreenMode::Alternate);
                pretty_assertions::assert_eq!(mode.mouse_protocol, None);
                Ok(())
            },
        )
        .await
    }

    #[tokio::test]
    async fn test_handle_client_message_when_split_pane_resyncs_quiet_deadline() -> rootcause::Result<()> {
        self::assert_layout_request_resyncs_quiet_deadline(|config| {
            let mut layout = self::layout(config)?;
            let tracked_pane = PaneId::new(1)?;
            layout.active_tab_mut()?.focus_pane(tracked_pane)?;
            Ok((layout, tracked_pane, self::shift_alt_key_request('V'), PaneId::new(3)?))
        })
        .await
    }

    #[tokio::test]
    async fn test_handle_client_message_when_tab_create_resyncs_quiet_deadline() -> rootcause::Result<()> {
        self::assert_layout_request_resyncs_quiet_deadline(|config| {
            let mut layout = self::layout(config)?;
            let tracked_pane = PaneId::new(1)?;
            layout.active_tab_mut()?.focus_pane(tracked_pane)?;
            Ok((layout, tracked_pane, self::shift_alt_key_request('E'), PaneId::new(3)?))
        })
        .await
    }

    #[tokio::test]
    async fn test_handle_client_message_when_focus_tab_resyncs_quiet_deadline() -> rootcause::Result<()> {
        self::assert_layout_request_resyncs_quiet_deadline(|config| {
            let mut layout = self::layout(config)?;
            let tracked_pane = PaneId::new(1)?;
            layout.active_tab_mut()?.focus_pane(tracked_pane)?;
            let target_pane = layout.create_tab(self::metadata("sh", 3))?;
            assert2::assert!(layout.focus_tab(TabId::new(1)?)?);
            Ok((
                layout,
                tracked_pane,
                ClientRequest::FocusTab(TabId::new(2)?),
                target_pane,
            ))
        })
        .await
    }

    #[tokio::test]
    #[expect(
        clippy::too_many_lines,
        reason = "the test covers scrollback open and restore through the routed client-message boundary"
    )]
    async fn test_handle_client_message_when_scrollback_open_and_restore_resync_quiet_deadline() -> rootcause::Result<()>
    {
        let tempdir = tempfile::tempdir()?;
        let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        config.shell_cmd = crate::server::test_helpers::shell_cmd("/bin/cat");
        let user_config = Arc::make_mut(&mut config.user_config);
        user_config.tracked_processes.push(TrackedProcess {
            id: TrackedProcessId::Codex,
            label: "cx",
            matchers: vec![ProcessMatcher::ExactExecutable("cat")],
            quiet_threshold: Duration::from_secs(3),
        });
        user_config.scrollback.editor = ScrollbackEditorConfig {
            program: "/bin/sh",
            args: &["-c", "cat \"$1\"; sleep 30", "muxr-test-scrollback-editor"],
        };
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        let tracked_pane = PaneId::new(1)?;
        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: vec![(tracked_pane, ShellCmd::with_args("/bin/cat", Vec::<String>::new())?)],
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        let then = Instant::now();
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            tracked_pane,
            &self::fg_tracked_process("cat"),
            then,
        );
        pane_tracked_processes.record_user_interaction(
            &layout,
            tracked_pane,
            TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_millis(1))?,
        )?;
        let mut timers = ClientTimers::new(&config)?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
        let focused_deadline = timers.tracked_process_quiet_sleep.deadline();
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let (mut event_writer, client_drain) = self::connect_client_event_drain(&config).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let mut heartbeat_started_at = None;
        let mut render_dirty = false;

        let keep_attached = crate::request_router::handle_client_message(
            SessionClientMessage::Request(self::shift_alt_key_request('S')),
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut heartbeat_started_at,
            &mut render_dirty,
        )
        .await?;

        assert2::assert!(keep_attached);
        assert2::assert!(state.scrollback_editor.is_some());
        assert2::assert!(timers.tracked_process_quiet_sleep.deadline() > focused_deadline);
        let disabled_deadline = timers.tracked_process_quiet_sleep.deadline();

        let keep_attached = crate::request_router::handle_client_message(
            SessionClientMessage::Request(self::shift_alt_key_request('W')),
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut heartbeat_started_at,
            &mut render_dirty,
        )
        .await?;

        assert2::assert!(keep_attached);
        assert2::assert!(state.scrollback_editor.is_none());
        pretty_assertions::assert_eq!(state.layout.active_pane_id()?, tracked_pane);
        assert2::assert!(timers.tracked_process_quiet_sleep.deadline() < disabled_deadline);
        assert2::assert!(!timers.tracked_process_quiet_sleep_deadline_has_passed());
        self::abort_client_drain(client_drain).await;
        Ok(())
    }

    #[tokio::test]
    async fn test_run_client_session_when_request_arrives_near_quiet_deadline_handles_request_and_quiet()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let then = Instant::now()
            .checked_sub(Duration::from_millis(2_950))
            .ok_or_else(|| rootcause::report!("test instant underflowed"))?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );

        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: Vec::new(),
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let listener = ServerListener::bind(&config.paths.socket)?;
        let (client_connection, server_connection) =
            tokio::try_join!(ClientConnection::connect(&config.paths.socket), listener.accept())?;
        let (mut client_reader, mut client_writer) = client_connection.split();
        let (mut request_reader, mut event_writer) = server_connection.split();
        client_writer.send_request(&ClientRequest::Ping).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let (_async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let session = self::run_client_session(
            &mut request_reader,
            &mut event_writer,
            &mut state,
            &mut async_pty_receiver,
        );
        let client = async {
            self::recv_until_pong_and_sidebar_state(&mut client_reader, pane_id, TrackedProcessState::Seen).await?;
            client_writer.send_request(&ClientRequest::Detach).await?;
            self::recv_until_detached(&mut client_reader).await?;
            Ok::<(), rootcause::Report>(())
        };

        let (session_result, client_result) = tokio::join!(session, client);

        session_result?;
        client_result?;
        Ok(())
    }

    #[tokio::test]
    async fn test_run_client_session_when_pty_output_arrives_before_quiet_deadline_keeps_busy() -> rootcause::Result<()>
    {
        let tempdir = tempfile::tempdir()?;
        let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        Arc::make_mut(&mut config.user_config)
            .tracked_processes
            .push(TrackedProcess {
                id: TrackedProcessId::Codex,
                label: "cx",
                matchers: vec![ProcessMatcher::ExactExecutable("cat")],
                quiet_threshold: Duration::from_secs(3),
            });
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let then = Instant::now()
            .checked_sub(Duration::from_millis(3_050))
            .ok_or_else(|| rootcause::report!("test instant underflowed"))?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("cat"),
            then,
        );

        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: vec![(pane_id, ShellCmd::with_args("/bin/cat", Vec::<String>::new())?)],
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        self::wait_for_runtime_fg_cmd(&runtimes, pane_id, "cat")?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let _baseline_dirty_panes = runtimes.take_screen_dirty_panes();
        runtimes.handle(pane_id)?.write_input(b"muxr-loop-boundary\n")?;
        self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "muxr-loop-boundary")?;
        let listener = ServerListener::bind(&config.paths.socket)?;
        let (client_connection, server_connection) =
            tokio::try_join!(ClientConnection::connect(&config.paths.socket), listener.accept())?;
        let (mut client_reader, mut client_writer) = client_connection.split();
        let (mut request_reader, mut event_writer) = server_connection.split();
        client_writer.send_request(&ClientRequest::Detach).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        async_pty_sender
            .send(SessionPaneOutputMessage::PaneOutputReady)
            .await
            .map_err(|error| rootcause::report!("failed to queue muxr test pty event").attach(format!("{error}")))?;
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let session = self::run_client_session(
            &mut request_reader,
            &mut event_writer,
            &mut state,
            &mut async_pty_receiver,
        );
        let client = async { self::recv_until_detached(&mut client_reader).await };

        let (session_result, client_result) = tokio::join!(session, client);

        session_result?;
        client_result?;
        pretty_assertions::assert_eq!(
            self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
            TrackedProcessState::Busy
        );
        Ok(())
    }

    #[tokio::test]
    #[expect(
        clippy::too_many_lines,
        reason = "the test builds the request-deferred queued-output quiet-boundary ordering end to end"
    )]
    async fn test_run_client_session_when_request_defers_quiet_drains_queued_output_first() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        Arc::make_mut(&mut config.user_config)
            .tracked_processes
            .push(TrackedProcess {
                id: TrackedProcessId::Codex,
                label: "cx",
                matchers: vec![ProcessMatcher::ExactExecutable("cat")],
                quiet_threshold: Duration::from_secs(3),
            });
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let then = Instant::now()
            .checked_sub(Duration::from_millis(3_050))
            .ok_or_else(|| rootcause::report!("test instant underflowed"))?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("cat"),
            then,
        );

        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: vec![(pane_id, ShellCmd::with_args("/bin/cat", Vec::<String>::new())?)],
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        self::wait_for_runtime_fg_cmd(&runtimes, pane_id, "cat")?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let _baseline_dirty_panes = runtimes.take_screen_dirty_panes();
        runtimes
            .handle(pane_id)?
            .write_input(b"muxr-loop-request-deferred-output\n")?;
        self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "muxr-loop-request-deferred-output")?;
        let listener = ServerListener::bind(&config.paths.socket)?;
        let (client_connection, server_connection) =
            tokio::try_join!(ClientConnection::connect(&config.paths.socket), listener.accept())?;
        let (mut client_reader, mut client_writer) = client_connection.split();
        let (mut request_reader, mut event_writer) = server_connection.split();
        client_writer.send_request(&ClientRequest::Ping).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        async_pty_sender
            .send(SessionPaneOutputMessage::PaneOutputReady)
            .await
            .map_err(|error| rootcause::report!("failed to queue muxr test pty event").attach(format!("{error}")))?;
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let session = self::run_client_session_after_output_turn(
            &mut request_reader,
            &mut event_writer,
            &mut state,
            &mut async_pty_receiver,
        );
        let client = async {
            self::recv_until_pong_rejecting_sidebar_state(&mut client_reader, pane_id, TrackedProcessState::Seen)
                .await?;
            client_writer.send_request(&ClientRequest::Detach).await?;
            loop {
                match self::recv_test_event(&mut client_reader).await? {
                    Some(ServerEvent::Detached) => break,
                    Some(ServerEvent::SidebarLayout(layout_snapshot)) => {
                        pretty_assertions::assert_eq!(
                            self::tracked_process_state(&layout_snapshot, pane_id)?,
                            TrackedProcessState::Busy
                        );
                    }
                    Some(_) => {}
                    None => return Err(rootcause::report!("expected muxr detach event")),
                }
            }
            Ok(())
        };

        let (session_result, client_result) = tokio::join!(session, client);

        session_result?;
        client_result?;
        pretty_assertions::assert_eq!(
            self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
            TrackedProcessState::Busy
        );
        assert2::assert!(matches!(async_pty_receiver.try_recv(), Err(TryRecvError::Empty)));
        Ok(())
    }

    #[tokio::test]
    #[expect(
        clippy::too_many_lines,
        reason = "the test builds the run-loop batch-limit then output quiet-boundary ordering"
    )]
    async fn test_run_client_session_when_batch_limit_precedes_queued_output_at_quiet_boundary_keeps_busy()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        Arc::make_mut(&mut config.user_config)
            .tracked_processes
            .push(TrackedProcess {
                id: TrackedProcessId::Codex,
                label: "cx",
                matchers: vec![ProcessMatcher::ExactExecutable("cat")],
                quiet_threshold: Duration::from_secs(3),
            });
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let then = Instant::now()
            .checked_sub(Duration::from_millis(3_050))
            .ok_or_else(|| rootcause::report!("test instant underflowed"))?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("cat"),
            then,
        );

        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: vec![(pane_id, ShellCmd::with_args("/bin/cat", Vec::<String>::new())?)],
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        self::wait_for_runtime_fg_cmd(&runtimes, pane_id, "cat")?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let _baseline_dirty_panes = runtimes.take_screen_dirty_panes();
        runtimes.handle(pane_id)?.write_input(b"muxr-loop-queued-boundary\n")?;
        self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "muxr-loop-queued-boundary")?;
        let listener = ServerListener::bind(&config.paths.socket)?;
        let (client_connection, server_connection) =
            tokio::try_join!(ClientConnection::connect(&config.paths.socket), listener.accept())?;
        let (mut client_reader, mut client_writer) = client_connection.split();
        let (mut request_reader, mut event_writer) = server_connection.split();
        client_writer.send_request(&ClientRequest::Detach).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        for _ in 0..QUIET_OUTPUT_DRAIN_BATCH_LIMIT {
            async_pty_sender
                .send(SessionPaneOutputMessage::PaneExited)
                .await
                .map_err(|error| {
                    rootcause::report!("failed to queue muxr test pty event").attach(format!("{error}"))
                })?;
        }
        async_pty_sender
            .send(SessionPaneOutputMessage::PaneExited)
            .await
            .map_err(|error| rootcause::report!("failed to queue muxr test pty event").attach(format!("{error}")))?;
        async_pty_sender
            .send(SessionPaneOutputMessage::PaneOutputReady)
            .await
            .map_err(|error| rootcause::report!("failed to queue muxr test pty event").attach(format!("{error}")))?;
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let session = self::run_client_session(
            &mut request_reader,
            &mut event_writer,
            &mut state,
            &mut async_pty_receiver,
        );
        let client = async { self::recv_until_detached(&mut client_reader).await };

        let (session_result, client_result) = tokio::join!(session, client);

        session_result?;
        client_result?;
        pretty_assertions::assert_eq!(
            self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
            TrackedProcessState::Busy
        );
        assert2::assert!(matches!(async_pty_receiver.try_recv(), Err(TryRecvError::Empty)));
        Ok(())
    }

    #[tokio::test]
    #[expect(
        clippy::too_many_lines,
        reason = "the test builds the queued PaneExited then PaneOutputReady boundary scenario end to end"
    )]
    async fn test_drain_queued_output_before_quiet_when_batch_limit_precedes_output_keeps_busy() -> rootcause::Result<()>
    {
        let tempdir = tempfile::tempdir()?;
        let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        Arc::make_mut(&mut config.user_config)
            .tracked_processes
            .push(TrackedProcess {
                id: TrackedProcessId::Codex,
                label: "cx",
                matchers: vec![ProcessMatcher::ExactExecutable("cat")],
                quiet_threshold: Duration::from_secs(3),
            });
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let then = Instant::now()
            .checked_sub(Duration::from_millis(3_050))
            .ok_or_else(|| rootcause::report!("test instant underflowed"))?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("cat"),
            then,
        );
        let mut timers = ClientTimers::new(&config)?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
        assert2::assert!(timers.tracked_process_quiet_sleep_deadline_has_passed());
        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: vec![(pane_id, ShellCmd::with_args("/bin/cat", Vec::<String>::new())?)],
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        self::wait_for_runtime_fg_cmd(&runtimes, pane_id, "cat")?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let _baseline_dirty_panes = runtimes.take_screen_dirty_panes();
        runtimes.handle(pane_id)?.write_input(b"muxr-queued-boundary\n")?;
        self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "muxr-queued-boundary")?;
        let (mut event_writer, client_drain) = self::connect_client_event_drain(&config).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        for _ in 0..QUIET_OUTPUT_DRAIN_BATCH_LIMIT {
            async_pty_sender
                .send(SessionPaneOutputMessage::PaneExited)
                .await
                .map_err(|error| {
                    rootcause::report!("failed to queue muxr test pty event").attach(format!("{error}"))
                })?;
        }
        async_pty_sender
            .send(SessionPaneOutputMessage::PaneExited)
            .await
            .map_err(|error| rootcause::report!("failed to queue muxr test pty event").attach(format!("{error}")))?;
        async_pty_sender
            .send(SessionPaneOutputMessage::PaneOutputReady)
            .await
            .map_err(|error| rootcause::report!("failed to queue muxr test pty event").attach(format!("{error}")))?;
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let mut render_dirty = false;

        pretty_assertions::assert_eq!(
            self::drain_queued_output_before_quiet(
                &mut async_pty_receiver,
                &mut event_writer,
                &mut state,
                &mut timers,
                &mut render_dirty,
            )
            .await?,
            OutputDrain::BatchLimitReached
        );
        assert2::assert!(timers.tracked_process_quiet_sleep_deadline_has_passed());

        pretty_assertions::assert_eq!(
            self::drain_queued_output_before_quiet(
                &mut async_pty_receiver,
                &mut event_writer,
                &mut state,
                &mut timers,
                &mut render_dirty,
            )
            .await?,
            OutputDrain::Output
        );

        pretty_assertions::assert_eq!(
            self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
            TrackedProcessState::Busy
        );
        assert2::assert!(!timers.tracked_process_quiet_sleep_deadline_has_passed());
        assert2::assert!(matches!(async_pty_receiver.try_recv(), Err(TryRecvError::Empty)));
        self::abort_client_drain(client_drain).await;
        Ok(())
    }

    #[tokio::test]
    async fn test_handle_pane_output_message_when_output_arrives_after_quiet_deadline_keeps_busy()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        Arc::make_mut(&mut config.user_config)
            .tracked_processes
            .push(TrackedProcess {
                id: TrackedProcessId::Codex,
                label: "cx",
                matchers: vec![ProcessMatcher::ExactExecutable("cat")],
                quiet_threshold: Duration::from_secs(3),
            });
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let then = Instant::now()
            .checked_sub(Duration::from_millis(3_050))
            .ok_or_else(|| rootcause::report!("test instant underflowed"))?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("cat"),
            then,
        );
        let mut timers = ClientTimers::new(&config)?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
        assert2::assert!(timers.tracked_process_quiet_sleep_deadline_has_passed());

        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: vec![(pane_id, ShellCmd::with_args("/bin/cat", Vec::<String>::new())?)],
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        self::wait_for_runtime_fg_cmd(&runtimes, pane_id, "cat")?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let _baseline_dirty_panes = runtimes.take_screen_dirty_panes();
        runtimes.handle(pane_id)?.write_input(b"muxr-boundary\n")?;
        self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "muxr-boundary")?;
        let (mut event_writer, client_drain) = self::connect_client_event_drain(&config).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let mut render_dirty = false;

        let keep_attached = crate::pty_output::handle_pane_output_message(
            Some(SessionPaneOutputMessage::PaneOutputReady),
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut render_dirty,
        )
        .await?;

        assert2::assert!(keep_attached);
        pretty_assertions::assert_eq!(
            self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
            TrackedProcessState::Busy
        );
        assert2::assert!(!timers.tracked_process_quiet_sleep_deadline_has_passed());
        pretty_assertions::assert_eq!(
            state.pane_tracked_processes.mark_quiet_deadlines(
                state.layout,
                self::instant_after(Instant::now(), Duration::from_secs(4))?
            )?,
            crate::pane::tracked_process::TrackedProcessAttention::Seen
        );
        self::abort_client_drain(client_drain).await;
        Ok(())
    }

    async fn assert_mouse_request_precedes_quiet_deadline_extends_busy(
        startup_script: &str,
        assert_mode: impl FnOnce(TerminalApplicationMode) -> rootcause::Result<()>,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        Arc::make_mut(&mut config.user_config)
            .tracked_processes
            .push(TrackedProcess {
                id: TrackedProcessId::Codex,
                label: "cx",
                matchers: vec![ProcessMatcher::ExactExecutable("cat")],
                quiet_threshold: Duration::from_millis(30),
            });
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: vec![(pane_id, ShellCmd::with_args("/bin/sh", ["-c", startup_script])?)],
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "ready")?;
        self::wait_for_runtime_fg_cmd(&runtimes, pane_id, "cat")?;
        assert_mode(runtimes.handle(pane_id)?.application_mode()?)?;
        let then = Instant::now()
            .checked_sub(Duration::from_millis(60))
            .ok_or_else(|| rootcause::report!("test instant underflowed"))?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("cat"),
            then,
        );
        let mut timers = ClientTimers::new(&config)?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
        assert2::assert!(timers.tracked_process_quiet_sleep_deadline_has_passed());
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let position = self::pane_position(&layout, &terminal_size, pane_id)?;
        let listener = ServerListener::bind(&config.paths.socket)?;
        let (_client_connection, server_connection) =
            tokio::try_join!(ClientConnection::connect(&config.paths.socket), listener.accept())?;
        let (_request_reader, mut event_writer) = server_connection.split();
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let mut heartbeat_started_at = None;
        let mut render_dirty = false;

        let keep_attached = crate::request_router::handle_client_message(
            SessionClientMessage::Request(ClientRequest::Mouse(ClientMouseEvent {
                button: 64,
                phase: ClientMouseEventPhase::Press,
                position,
            })),
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut heartbeat_started_at,
            &mut render_dirty,
        )
        .await?;

        assert2::assert!(keep_attached);
        pretty_assertions::assert_eq!(
            self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
            TrackedProcessState::Busy
        );
        assert2::assert!(!timers.tracked_process_quiet_sleep_deadline_has_passed());
        Ok(())
    }

    async fn assert_layout_request_resyncs_quiet_deadline(
        setup: impl FnOnce(&ServerConfig) -> rootcause::Result<(SessionLayout, PaneId, ClientRequest, PaneId)>,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        config.shell_cmd = crate::server::test_helpers::shell_cmd("/bin/cat");
        crate::session::files::prepare_session_dirs(&config.paths)?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let (mut layout, tracked_pane_id, request, expected_active_pane) = setup(&config)?;
        pretty_assertions::assert_eq!(layout.active_pane_id()?, tracked_pane_id);
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            tracked_pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        pane_tracked_processes.record_user_interaction(
            &layout,
            tracked_pane_id,
            TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_secs(2))?,
        )?;
        let mut timers = ClientTimers::new(&config)?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
        let focused_deadline = timers.tracked_process_quiet_sleep.deadline();
        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: Vec::new(),
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        crate::screen_render::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        let (layout_snapshot, pane_regions, mut render_composer, _render_baseline) =
            crate::screen_render::initial_client_render(
                &config,
                &mut layout,
                &runtimes,
                &pane_tracked_processes,
                &terminal_size,
            )?;
        let (mut event_writer, client_drain) = self::connect_client_event_drain(&config).await?;
        let delete_sessions = DeleteSessions::default();
        let (pty_event_sender, _pty_event_receiver) = mpsc::sync_channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
        let mut sink_guards = Vec::new();
        let mut state = ClientSessionState {
            pane_tracked_processes,
            config: &config,
            delete_sessions: &delete_sessions,
            input_mode: ServerInputMode::Normal,
            last_layout_snapshot: layout_snapshot,
            layout: &mut layout,
            pane_fullscreen: PaneFullscreen::default(),
            pane_regions,
            pty_event_sender: &pty_event_sender,
            render_composer: &mut render_composer,
            runtimes: &mut runtimes,
            scrollback_editor: None,
            sink_guards: &mut sink_guards,
            terminal_size,
        };
        let mut heartbeat_started_at = None;
        let mut render_dirty = false;

        let keep_attached = crate::request_router::handle_client_message(
            SessionClientMessage::Request(request),
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut heartbeat_started_at,
            &mut render_dirty,
        )
        .await?;

        assert2::assert!(keep_attached);
        pretty_assertions::assert_eq!(state.layout.active_pane_id()?, expected_active_pane);
        assert2::assert!(timers.tracked_process_quiet_sleep.deadline() < focused_deadline);
        self::abort_client_drain(client_drain).await;
        Ok(())
    }

    fn layout(config: &ServerConfig) -> rootcause::Result<SessionLayout> {
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.split_active_pane(
            config.user_config.layout,
            self::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        Ok(layout)
    }

    fn pane_position(
        layout: &SessionLayout,
        terminal_size: &TerminalSize,
        pane_id: PaneId,
    ) -> rootcause::Result<ClientMousePosition> {
        let region = layout
            .pane_regions(terminal_size)?
            .into_iter()
            .find(|region| region.id == pane_id)
            .ok_or_else(|| {
                rootcause::report!("muxr test pane region is missing").attach(format!("pane_id={pane_id}"))
            })?;
        Ok(ClientMousePosition {
            row: region.area.origin.row,
            col: region.area.origin.col,
        })
    }

    fn shift_alt_key_request(ch: char) -> ClientRequest {
        ClientRequest::Key(ClientKey {
            code: ClientKeyCode::Char(ch),
            modifiers: ClientKeyModifiers::SHIFT_ALT,
            raw_bytes: format!("\x1b{ch}").into_bytes(),
        })
    }

    fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }

    fn fg_tracked_process(executable: &str) -> PaneCmdObservation {
        PaneCmdObservation::FgCmd {
            cmd: PaneCmd {
                executable: executable.to_owned(),
                path: None,
                pid: 42,
            },
        }
    }

    async fn recv_test_event(reader: &mut ClientEventReader) -> rootcause::Result<Option<ServerEvent>> {
        tokio::time::timeout(Duration::from_secs(1), reader.recv_event())
            .await
            .map_err(|error| {
                rootcause::report!("timed out waiting for muxr test client event").attach(format!("{error}"))
            })?
    }

    async fn recv_until_pong_and_sidebar_state(
        reader: &mut ClientEventReader,
        pane_id: PaneId,
        expected_state: TrackedProcessState,
    ) -> rootcause::Result<()> {
        let mut pong = false;
        let mut sidebar = false;
        while !pong || !sidebar {
            match self::recv_test_event(reader).await? {
                Some(ServerEvent::Pong) => pong = true,
                Some(ServerEvent::SidebarLayout(layout_snapshot)) => {
                    pretty_assertions::assert_eq!(
                        self::tracked_process_state(&layout_snapshot, pane_id)?,
                        expected_state
                    );
                    sidebar = true;
                }
                Some(_event) => {}
                None => {
                    return Err(rootcause::report!(
                        "muxr test client disconnected before expected events"
                    ));
                }
            }
        }
        Ok(())
    }

    async fn recv_until_pong_rejecting_sidebar_state(
        reader: &mut ClientEventReader,
        pane_id: PaneId,
        rejected_state: TrackedProcessState,
    ) -> rootcause::Result<()> {
        loop {
            match self::recv_test_event(reader).await? {
                Some(ServerEvent::Pong) => return Ok(()),
                Some(ServerEvent::SidebarLayout(layout_snapshot)) => {
                    let state = self::tracked_process_state(&layout_snapshot, pane_id)?;
                    if state == rejected_state {
                        return Err(
                            rootcause::report!("unexpected muxr tracked-process sidebar state before pong")
                                .attach(format!("pane_id={pane_id} state={state:?}")),
                        );
                    }
                }
                Some(_event) => {}
                None => return Err(rootcause::report!("muxr test client disconnected before pong")),
            }
        }
    }

    async fn recv_until_detached(reader: &mut ClientEventReader) -> rootcause::Result<()> {
        loop {
            match self::recv_test_event(reader).await? {
                Some(ServerEvent::Detached) => return Ok(()),
                Some(_event) => {}
                None => return Err(rootcause::report!("muxr test client disconnected before detach")),
            }
        }
    }

    async fn connect_client_event_drain(
        config: &ServerConfig,
    ) -> rootcause::Result<(ServerEventWriter, tokio::task::JoinHandle<()>)> {
        let listener = ServerListener::bind(&config.paths.socket)?;
        let (client_connection, server_connection) =
            tokio::try_join!(ClientConnection::connect(&config.paths.socket), listener.accept())?;
        let (mut client_reader, _client_writer) = client_connection.split();
        let client_drain =
            tokio::spawn(async move { while let Ok(Some(_event)) = client_reader.recv_event().await {} });
        let (_request_reader, event_writer) = server_connection.split();
        Ok((event_writer, client_drain))
    }

    async fn abort_client_drain(handle: tokio::task::JoinHandle<()>) {
        handle.abort();
        let _ = tokio::time::timeout(Duration::from_secs(1), handle).await;
    }

    fn tracked_process_state(
        layout_snapshot: &LayoutSnapshot,
        pane_id: PaneId,
    ) -> rootcause::Result<TrackedProcessState> {
        layout_snapshot
            .tabs()
            .iter()
            .flat_map(muxr_core::TabSnapshot::panes)
            .find(|pane| pane.id == pane_id)
            .map(|pane| pane.tracked_process_state)
            .ok_or_else(|| rootcause::report!("expected muxr pane snapshot").attach(format!("pane_id={pane_id}")))
    }

    fn tracked_process_snapshot_state(
        snapshot: &crate::pane::tracked_process::PaneTrackedProcessSnapshot,
        pane_id: PaneId,
    ) -> rootcause::Result<TrackedProcessState> {
        snapshot
            .panes()
            .find(|(snapshot_pane_id, _pane)| *snapshot_pane_id == pane_id)
            .map(|(_pane_id, pane)| pane.state())
            .ok_or_else(|| {
                rootcause::report!("expected muxr tracked process snapshot").attach(format!("pane_id={pane_id}"))
            })
    }

    fn instant_after(instant: Instant, duration: Duration) -> rootcause::Result<Instant> {
        instant
            .checked_add(duration)
            .ok_or_else(|| rootcause::report!("test instant overflowed"))
    }

    fn wait_for_pane_exit(runtimes: &PaneRuntimes, pane_id: PaneId) -> rootcause::Result<()> {
        let started_at = Instant::now();
        while !runtimes.handle(pane_id)?.has_exited() {
            if started_at.elapsed() > Duration::from_secs(2) {
                return Err(rootcause::report!("timed out waiting for muxr test pane exit")
                    .attach(format!("pane_id={pane_id}")));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn wait_for_runtime_fg_cmd(runtimes: &PaneRuntimes, pane_id: PaneId, expected: &str) -> rootcause::Result<()> {
        let started_at = Instant::now();
        loop {
            let snapshot = PaneCmdSnapshot::try_from(&runtimes.handle(pane_id)?)?;
            if let PaneCmdObservation::FgCmd { cmd } = PaneCmdObservation::from(&snapshot)
                && cmd.executable == expected
            {
                return Ok(());
            }
            if started_at.elapsed() > Duration::from_secs(2) {
                return Err(rootcause::report!("timed out waiting for muxr runtime fg cmd")
                    .attach(format!("expected={expected}")));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_runtime_snapshot_contains(
        runtimes: &PaneRuntimes,
        pane_id: PaneId,
        needle: &str,
    ) -> rootcause::Result<()> {
        let started_at = Instant::now();
        loop {
            let snapshot = runtimes.handle(pane_id)?.render_snapshot()?;
            if self::snapshot_text(&snapshot).contains(needle) {
                return Ok(());
            }
            if started_at.elapsed() > Duration::from_secs(2) {
                return Err(rootcause::report!("timed out waiting for muxr runtime snapshot")
                    .attach(format!("needle={needle}")));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn snapshot_text(snapshot: &TerminalSnapshot) -> String {
        snapshot
            .rows()
            .iter()
            .flat_map(muxr_core::RenderRowSpan::cells)
            .map(muxr_core::RenderCell::text)
            .collect()
    }
}
