use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use muxr_config::MuxrConfig;
use muxr_config::ProcessMatcher;
use muxr_config::ScreenObservationConfig;
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
use regex::Regex;
use test_that::prelude::*;

use super::*;
use crate::event_writer::ServerEventSink;
use crate::pane::cmd::PaneCmd;
use crate::pane::cmd::PaneCmdObservation;
use crate::pane::cmd::PaneCmdSnapshot;
use crate::pane::split::PaneSplitAxis;
use crate::pane::tracked_process::PaneTrackedProcessSnapshot;
use crate::pane::tracked_process::TrackedProcessAttention;
use crate::pane::tracked_process::TrackedProcessChanges;
use crate::pane::tracked_process::TrackedProcessStateChange;
use crate::pane::tracked_process::TrackedProcessUserInteraction;
use crate::pty::ShellCmd;
use crate::session::start_seed::SessionStartSeed;
use crate::state::PaneTreeRightPane;
use crate::state::SessionMetadata;
use crate::terminal::TerminalApplicationMode;
use crate::terminal::TerminalScreenMode;
use crate::terminal::TerminalSnapshot;

mod tracked_process;

const TEST_RUNTIME_READY_TIMEOUT: Duration = Duration::from_secs(5);

#[rstest::rstest]
#[case::running_right_nvim(true, NvimState::Running, true)]
#[case::right_pane_without_nvim(true, NvimState::NotRunning, false)]
#[case::right_pane_with_unknown_state(true, NvimState::Unknown, false)]
#[case::missing_right_pane(false, NvimState::Running, false)]
fn test_open_file_pane_route_reuses_only_a_running_right_nvim_pane(
    #[case] has_right_pane: bool,
    #[case] nvim_state: NvimState,
    #[case] reuses_right_pane: bool,
) -> rootcause::Result<()> {
    let right_pane_id = PaneId::new(2)?;
    let right_pane = has_right_pane.then_some(PaneTreeRightPane::Pane(right_pane_id));
    test_that::assert_that!(
        self::open_file_pane_route_for_right_pane(right_pane.unwrap_or(PaneTreeRightPane::Missing), nvim_state),
        eq(if reuses_right_pane {
            OpenFilePaneRoute::ExistingNvim(right_pane_id)
        } else {
            OpenFilePaneRoute::NewRightSplit
        })
    );
    Ok(())
}

#[tokio::test]
async fn test_pty_event_bridge_forwards_events_in_order_and_stops_when_async_receiver_drops() -> rootcause::Result<()> {
    let (pty_event_sender, pty_event_receiver) = self::pty_event_channel();
    let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
    let (bridge_done_sender, bridge_done_receiver) = mpsc::channel();
    let bridge_handle = thread::spawn(move || {
        self::forward_pty_events_to_async(&pty_event_receiver, &async_pty_sender);
        let _sent = bridge_done_sender.send(());
    });

    test_that::assert_that!(
        pty_event_sender.send_timeout(PtyEvent::OutputReady, Duration::from_secs(1)),
        ok(eq(()))
    );
    test_that::assert_that!(
        pty_event_sender.send_timeout(PtyEvent::Exited, Duration::from_secs(1)),
        ok(eq(()))
    );

    test_that::assert_that!(
        self::recv_pty_bridge_event(&mut async_pty_receiver, "output ready").await?,
        eq(SessionPaneOutputMessage::PaneOutputReady)
    );
    test_that::assert_that!(
        self::recv_pty_bridge_event(&mut async_pty_receiver, "exited").await?,
        eq(SessionPaneOutputMessage::PaneExited)
    );

    drop(async_pty_receiver);
    test_that::assert_that!(
        pty_event_sender.send_timeout(PtyEvent::OutputReady, Duration::from_secs(1)),
        ok(eq(()))
    );
    bridge_done_receiver
        .recv_timeout(Duration::from_secs(1))
        .map_err(|error| report!("muxr pty event bridge did not stop after async receiver drop").attach(error))?;
    bridge_handle
        .join()
        .map_err(|_| report!("muxr pty event bridge test thread panicked"))?;
    Ok(())
}

async fn recv_pty_bridge_event(
    async_pty_receiver: &mut tokio::sync::mpsc::Receiver<SessionPaneOutputMessage>,
    label: &str,
) -> rootcause::Result<SessionPaneOutputMessage> {
    // Bridge regressions should fail this test instead of leaving CI parked on an unbounded receive.
    tokio::time::timeout(Duration::from_secs(1), async_pty_receiver.recv())
        .await
        .map_err(|error| {
            report!("timed out waiting for muxr pty bridge event")
                .attach(error)
                .attach(label.to_owned())
        })?
        .ok_or_else(|| report!("muxr pty event bridge closed before receiving event").attach(label.to_owned()))
}
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
        &self::fg_tracked_process("claude"),
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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    runtimes.handle(pane_id)?.write_input(b"exit\n")?;
    self::wait_for_pane_exit(&runtimes, pane_id)?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut render_dmg = ClientRenderDmg::Clean;

    let keep_attached = crate::pty_output::handle_pane_output_message(
        Some(SessionPaneOutputMessage::PaneExited),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(state.layout.active_pane_id()?, eq(other_pane_id));
    test_that::assert_that!(
        state.pane_tracked_processes.attention_pane_ids(state.layout),
        eq(Vec::new())
    );
    test_that::assert_that!(
        state.pane_tracked_processes.next_quiet_deadline(state.layout)?,
        eq(None)
    );
    test_that::assert_that!(
        timers.tracked_process_quiet_sleep.deadline() > focused_deadline,
        eq(true)
    );
    self::abort_client_drain(client_drain).await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::too_many_lines,
    reason = "the test keeps the multi-pane reap, client-resource cleanup, and stale tracked-state assertions together"
)]
async fn test_handle_pane_output_message_when_batch_reap_removes_panes_drops_client_resources() -> rootcause::Result<()>
{
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
            &self::fg_tracked_process("claude"),
            then,
        );
    }
    let mut timers = ClientTimers::new(&config)?;
    timers.schedule_cmd_handoff_sample(first_exited_pane)?;
    timers.schedule_cmd_handoff_sample(second_exited_pane)?;
    timers.schedule_cmd_handoff_sample(surviving_pane)?;
    timers.schedule_output_activity_samples(&[first_exited_pane, second_exited_pane, surviving_pane])?;

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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
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
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut render_dmg = ClientRenderDmg::Clean;

    let keep_attached = crate::pty_output::handle_pane_output_message(
        Some(SessionPaneOutputMessage::PaneExited),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut render_dmg,
    )
    .await?;

    let sink_guard_pane_ids = state.sink_guards.iter().map(|sink| sink.pane_id).collect::<Vec<_>>();
    let snapshot = state.pane_tracked_processes.snapshot(state.layout);
    let tracked_process_pane_ids = snapshot.panes().map(|(pane_id, _pane)| pane_id).collect::<Vec<_>>();
    let removed_tracked_processes = (
        state.pane_tracked_processes.remove_pane(first_exited_pane),
        state.pane_tracked_processes.remove_pane(second_exited_pane),
    );
    tokio::time::advance(Duration::from_millis(500)).await;
    // Batch reap returns every removed pane; this end-to-end assertion keeps all related client resources in sync.
    test_that::assert_that!(
        (
            keep_attached,
            state.layout.pane_ids(),
            state.runtimes.pane_ids(),
            sink_guard_pane_ids,
            tracked_process_pane_ids,
            removed_tracked_processes,
            self::tracked_process_snapshot_state(&snapshot, surviving_pane)?,
            timers.take_cmd_handoff_sample_panes()?,
            timers.take_due_output_activity_sample_panes()?,
        ),
        eq((
            ClientSessionFlow::Continue,
            vec![surviving_pane],
            vec![surviving_pane],
            vec![surviving_pane],
            vec![surviving_pane],
            (TrackedProcessChanges::default(), TrackedProcessChanges::default()),
            TrackedProcessState::Busy,
            vec![surviving_pane],
            vec![surviving_pane],
        ))
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
        &self::fg_tracked_process("claude"),
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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut heartbeat_started_at = None;
    let mut render_dmg = ClientRenderDmg::Clean;
    let keep_attached = crate::request_router::handle_client_message(
        SessionClientMessage::Request(ClientRequest::FocusPaneAt(target_position)),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut heartbeat_started_at,
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(state.layout.active_pane_id()?, eq(other_pane_id));
    test_that::assert_that!(
        timers.tracked_process_quiet_sleep.deadline() < focused_deadline,
        eq(true)
    );
    self::abort_client_drain(client_drain).await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the test keeps close-pane focus, resource cleanup, and handoff timer assertions together"
)]
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
        &self::fg_tracked_process("claude"),
        then,
    );
    test_that::assert_that!(
        pane_tracked_processes.mark_quiet_deadlines(&layout, self::instant_after(then, Duration::from_secs(3))?,)?,
        eq(TrackedProcessAttention::Unseen {
            pane_ids: vec![fallback_pane_id]
        })
    );
    pane_tracked_processes.observe_pane_cmd(
        config.user_config.as_ref(),
        active_pane_id,
        &self::fg_tracked_process("claude"),
        then,
    );
    let mut timers = ClientTimers::new(&config)?;
    timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
    timers.schedule_cmd_handoff_sample(active_pane_id)?;
    timers.schedule_cmd_handoff_sample(fallback_pane_id)?;

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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = super::attach_pane_sinks(&runtimes, &pty_event_sender)?;
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut heartbeat_started_at = None;
    let mut render_dmg = ClientRenderDmg::Clean;

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
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(state.layout.active_pane_id()?, eq(fallback_pane_id));
    let sink_guard_pane_ids = state.sink_guards.iter().map(|sink| sink.pane_id).collect::<Vec<_>>();
    let snapshot = state.pane_tracked_processes.snapshot(state.layout);
    let tracked_process_pane_ids = snapshot.panes().map(|(pane_id, _pane)| pane_id).collect::<Vec<_>>();
    let removed_tracked_process = state.pane_tracked_processes.remove_pane(active_pane_id);
    let fallback = snapshot
        .panes()
        .find(|(pane_id, _pane)| *pane_id == fallback_pane_id)
        .map(|(_pane_id, pane)| pane)
        .ok_or_else(|| rootcause::report!("expected fallback pane tracked state"))?;
    test_that::assert_that!(
        (
            state.layout.pane_ids(),
            state.runtimes.pane_ids(),
            sink_guard_pane_ids,
            tracked_process_pane_ids,
            removed_tracked_process,
            fallback.state(),
            timers.take_cmd_handoff_sample_panes()?,
        ),
        eq((
            vec![fallback_pane_id],
            vec![fallback_pane_id],
            vec![fallback_pane_id],
            vec![fallback_pane_id],
            TrackedProcessChanges::default(),
            TrackedProcessState::Seen,
            vec![fallback_pane_id],
        ))
    );
    self::abort_client_drain(client_drain).await;
    Ok(())
}

#[tokio::test]
async fn test_handle_client_message_when_input_prompt_submit_marks_seen_tracked_process_busy() -> rootcause::Result<()>
{
    self::assert_prompt_submit_marks_seen_tracked_process_busy(ClientRequest::Input(b"\r".to_vec())).await
}

#[tokio::test]
async fn test_handle_client_message_when_key_prompt_submit_marks_seen_tracked_process_busy() -> rootcause::Result<()> {
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
        &self::fg_tracked_process("claude"),
        then,
    );
    test_that::assert_that!(
        pane_tracked_processes.mark_quiet_deadlines(&layout, self::instant_after(then, Duration::from_secs(3))?,)?,
        eq(TrackedProcessAttention::Seen)
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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    test_that::assert_that!(
        self::tracked_process_state(&layout_snapshot, pane_id)?,
        eq(TrackedProcessState::Seen)
    );
    let listener = ServerListener::bind(&config.paths.socket)?;
    let (client_connection, server_connection) =
        tokio::try_join!(ClientConnection::connect(&config.paths.socket), listener.accept())?;
    let (mut client_reader, _client_writer) = client_connection.split();
    let (_request_reader, event_writer) = server_connection.split();
    let mut event_writer = render_worker.attach_writer(event_writer, config.client_write_timeout)?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut heartbeat_started_at = None;
    let mut render_dmg = ClientRenderDmg::Clean;

    let keep_attached = crate::request_router::handle_client_message(
        SessionClientMessage::Request(request),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut heartbeat_started_at,
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    let Some(ServerEvent::SidebarLayout(layout_snapshot)) = self::recv_test_event(&mut client_reader).await? else {
        return Err(rootcause::report!(
            "expected muxr prompt submit tracked-process layout update"
        ));
    };
    test_that::assert_that!(
        self::tracked_process_state(&layout_snapshot, pane_id)?,
        eq(TrackedProcessState::Busy)
    );
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Pending));
    Ok(())
}

#[tokio::test]
async fn test_handle_client_message_when_focused_input_precedes_quiet_deadline_extends_busy() -> rootcause::Result<()> {
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

async fn assert_focused_may_echo_request_precedes_quiet_deadline_extends_busy(
    request: ClientRequest,
) -> rootcause::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
    Arc::make_mut(&mut config.user_config)
        .tracked_processes
        .push(self::tracked_cat_process("cl", Duration::from_millis(30)));
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
    let completed = format!("{}Worked for 22m 38s • 11:37\n", "\n".repeat(24));
    runtimes.handle(pane_id)?.write_input(completed.as_bytes())?;
    self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "Worked for 22m 38s • 11:37")?;
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
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Elapsed));
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut heartbeat_started_at = None;
    let mut render_dmg = ClientRenderDmg::Clean;

    let keep_attached = crate::request_router::handle_client_message(
        SessionClientMessage::Request(request),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut heartbeat_started_at,
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(TrackedProcessState::Busy)
    );
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Pending));

    tokio::time::sleep(Duration::from_millis(45)).await;
    let keep_attached = self::handle_session_runtime_timer_message(
        SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached,
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut heartbeat_started_at,
        &mut render_dmg,
    )
    .await?;
    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(TrackedProcessState::Seen)
    );
    self::abort_client_drain(client_drain).await;
    Ok(())
}

fn assert_ignored_enter_preserves_completion(
    state: &mut ClientSessionState<'_>,
    pane_id: PaneId,
    initial_state: TrackedProcessState,
    due: Instant,
) -> rootcause::Result<()> {
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(initial_state)
    );
    state
        .pane_tracked_processes
        .record_cached_visible_activity(state.runtimes, &[pane_id], due)?;
    let later = self::instant_after(due, Duration::from_secs(30))?;
    state.pane_tracked_processes.guard_quiet_deadlines(
        &state.config.user_config,
        state.layout,
        state.runtimes,
        later,
    )?;
    let expected = if initial_state == TrackedProcessState::Busy {
        TrackedProcessAttention::Seen
    } else {
        TrackedProcessAttention::Unchanged
    };
    test_that::assert_that!(
        state.pane_tracked_processes.mark_quiet_deadlines(state.layout, later)?,
        eq(expected)
    );
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(TrackedProcessState::Seen)
    );
    test_that::assert_that!(
        state.pane_tracked_processes.next_quiet_deadline(state.layout)?,
        eq(None)
    );
    Ok(())
}

fn assert_submission_waits_for_fresh_completion(
    state: &mut ClientSessionState<'_>,
    pane_id: PaneId,
) -> rootcause::Result<()> {
    let retry = Instant::now();
    test_that::assert_that!(
        state.pane_tracked_processes.next_quiet_deadline(state.layout)?,
        eq(None)
    );
    // Repainting the pre-submission footer must not start green or arm attention.
    state
        .pane_tracked_processes
        .record_cached_visible_activity(state.runtimes, &[pane_id], retry)?;
    test_that::assert_that!(
        state.pane_tracked_processes.next_quiet_deadline(state.layout)?,
        eq(None)
    );
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(TrackedProcessState::Seen)
    );
    // A new completion is sufficient even when Working was never sampled.
    let output = format!("{}Worked for 2s • 14:19\n", "\n".repeat(24));
    state.runtimes.handle(pane_id)?.write_input(output.as_bytes())?;
    self::wait_for_runtime_snapshot_contains(state.runtimes, pane_id, "Worked for 2s • 14:19")?;
    state
        .pane_tracked_processes
        .record_cached_visible_activity(state.runtimes, &[pane_id], retry)?;
    let completed = self::instant_after(retry, Duration::from_secs(3))?;
    state.pane_tracked_processes.guard_quiet_deadlines(
        &state.config.user_config,
        state.layout,
        state.runtimes,
        completed,
    )?;
    test_that::assert_that!(
        state
            .pane_tracked_processes
            .mark_quiet_deadlines(state.layout, completed)?,
        eq(TrackedProcessAttention::Seen)
    );
    Ok(())
}

async fn assert_repainted_completion_settles(
    state: &mut ClientSessionState<'_>,
    event_writer: &mut impl ServerEventSink,
    timers: &mut ClientTimers,
    pane_id: PaneId,
    expected_state: TrackedProcessState,
) -> rootcause::Result<()> {
    let mut render_dmg = ClientRenderDmg::Clean;
    test_that::assert_that!(
        crate::pty_output::handle_pane_output_message(
            Some(SessionPaneOutputMessage::PaneOutputReady),
            event_writer,
            state,
            timers,
            &mut render_dmg,
        )
        .await?,
        eq(ClientSessionFlow::Continue)
    );
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Elapsed));
    let mut heartbeat_started_at = None;
    test_that::assert_that!(
        self::handle_session_runtime_timer_message(
            SessionRuntimeTimerMessage::TrackedProcessQuietDeadlineReached,
            event_writer,
            state,
            timers,
            &mut heartbeat_started_at,
            &mut render_dmg,
        )
        .await?,
        eq(ClientSessionFlow::Continue)
    );
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(expected_state)
    );
    let redraw = format!("{}idle redraw\n", "\n".repeat(24));
    state.runtimes.handle(pane_id)?.write_input(redraw.as_bytes())?;
    self::wait_for_runtime_snapshot_contains(state.runtimes, pane_id, "idle redraw")?;
    state
        .pane_tracked_processes
        .record_cached_visible_activity(state.runtimes, &[pane_id], Instant::now())?;
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(expected_state)
    );
    Ok(())
}

#[tokio::test]
async fn test_handle_client_message_when_mouse_forward_precedes_quiet_deadline_extends_busy() -> rootcause::Result<()> {
    self::assert_mouse_request_precedes_quiet_deadline_extends_busy(
        "printf '\\033[?1002h\\033[?1006h%s%s\\n' re ady; exec /bin/cat",
        |mode| {
            test_that::assert_that!(mode.mouse_protocol.as_ref(), some(anything()));
            Ok(())
        },
    )
    .await
}

#[tokio::test]
async fn test_handle_client_message_when_faux_scroll_precedes_quiet_deadline_extends_busy() -> rootcause::Result<()> {
    self::assert_mouse_request_precedes_quiet_deadline_extends_busy(
        "printf '\\033[?1049h%s%s\\n' re ady; exec /bin/cat",
        |mode| {
            test_that::assert_that!(mode.screen_mode, eq(TerminalScreenMode::Alternate));
            test_that::assert_that!(mode.mouse_protocol, eq(None));
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
        test_that::assert_that!(
            layout.focus_tab(TabId::new(1)?)?,
            eq(crate::tab::focus::TabFocusChange::Changed)
        );
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
async fn test_handle_client_message_when_scrollback_open_and_restore_resync_quiet_deadline() -> rootcause::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
    config.shell_cmd = crate::server::test_helpers::shell_cmd("/bin/cat");
    let user_config = Arc::make_mut(&mut config.user_config);
    user_config
        .tracked_processes
        .push(self::tracked_cat_process("cl", Duration::from_secs(3)));
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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut heartbeat_started_at = None;
    let mut render_dmg = ClientRenderDmg::Clean;

    let keep_attached = crate::request_router::handle_client_message(
        SessionClientMessage::Request(self::shift_alt_key_request('S')),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut heartbeat_started_at,
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(state.scrollback_editor.as_ref(), some(anything()));
    test_that::assert_that!(
        timers.tracked_process_quiet_sleep.deadline() > focused_deadline,
        eq(true)
    );
    let disabled_deadline = timers.tracked_process_quiet_sleep.deadline();

    let keep_attached = crate::request_router::handle_client_message(
        SessionClientMessage::Request(self::shift_alt_key_request('W')),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut heartbeat_started_at,
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(state.scrollback_editor.as_ref(), none());
    test_that::assert_that!(state.layout.active_pane_id()?, eq(tracked_pane));
    test_that::assert_that!(
        timers.tracked_process_quiet_sleep.deadline() < disabled_deadline,
        eq(true)
    );
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Pending));
    self::abort_client_drain(client_drain).await;
    Ok(())
}

#[tokio::test]
async fn test_run_client_session_when_request_arrives_near_quiet_deadline_handles_request_and_quiet()
-> rootcause::Result<()> {
    let TrackedCatRuntimeFixture {
        _tempdir,
        config,
        terminal_size,
        mut layout,
        pane_id,
        mut runtimes,
    } = self::tracked_cat_runtime_fixture()?;
    let working = format!("{}Working (1s • esc to interrupt)\n", "\n".repeat(24));
    runtimes.handle(pane_id)?.write_input(working.as_bytes())?;
    self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "Working (1s • esc to interrupt)")?;
    let then = Instant::now()
        .checked_sub(Duration::from_millis(2_950))
        .ok_or_else(|| rootcause::report!("test instant underflowed"))?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    pane_tracked_processes.observe_runtime_pane_cmds(config.user_config.as_ref(), &runtimes, &[pane_id], then)?;
    let completed = format!("{}Worked for 22m 38s • 11:37\n", "\n".repeat(24));
    runtimes.handle(pane_id)?.write_input(completed.as_bytes())?;
    self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "Worked for 22m 38s • 11:37")?;
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
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
    let (mut request_reader, event_writer) = server_connection.split();
    let mut event_writer = render_worker.attach_writer(event_writer, config.client_write_timeout)?;
    client_writer.send_request(&ClientRequest::Ping).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
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
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let session = self::run_test_client_session(
        &mut request_reader,
        &mut event_writer,
        &mut state,
        &mut async_pty_receiver,
        ClientSessionSelectBias::Output,
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
async fn test_run_client_session_when_pty_output_arrives_before_quiet_deadline_keeps_busy() -> rootcause::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
    Arc::make_mut(&mut config.user_config)
        .tracked_processes
        .push(self::tracked_cat_process("cl", Duration::from_secs(3)));
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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
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
    let (mut request_reader, event_writer) = server_connection.split();
    let mut event_writer = render_worker.attach_writer(event_writer, config.client_write_timeout)?;
    client_writer.send_request(&ClientRequest::Detach).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
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
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let session = self::run_test_client_session(
        &mut request_reader,
        &mut event_writer,
        &mut state,
        &mut async_pty_receiver,
        ClientSessionSelectBias::Output,
    );
    let client = async { self::recv_until_detached(&mut client_reader).await };

    let (session_result, client_result) = tokio::join!(session, client);

    session_result?;
    client_result?;
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(TrackedProcessState::Busy)
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
        .push(self::tracked_cat_process("cl", Duration::from_secs(3)));
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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
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
    let (mut request_reader, event_writer) = server_connection.split();
    let mut event_writer = render_worker.attach_writer(event_writer, config.client_write_timeout)?;
    client_writer.send_request(&ClientRequest::Ping).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
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
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let session = self::run_test_client_session(
        &mut request_reader,
        &mut event_writer,
        &mut state,
        &mut async_pty_receiver,
        ClientSessionSelectBias::Request,
    );
    let client = async {
        self::recv_until_pong_rejecting_sidebar_state(&mut client_reader, pane_id, TrackedProcessState::Seen).await?;
        client_writer.send_request(&ClientRequest::Detach).await?;
        loop {
            match self::recv_test_event(&mut client_reader).await? {
                Some(ServerEvent::Detached) => break,
                Some(ServerEvent::SidebarLayout(layout_snapshot)) => {
                    test_that::assert_that!(
                        self::tracked_process_state(&layout_snapshot, pane_id)?,
                        eq(TrackedProcessState::Busy)
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
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(TrackedProcessState::Busy)
    );
    test_that::assert_that!(
        async_pty_receiver.try_recv(),
        err(matches_pattern!(TryRecvError::Empty))
    );
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
        .push(self::tracked_cat_process("cl", Duration::from_secs(3)));
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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
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
    let (mut request_reader, event_writer) = server_connection.split();
    let mut event_writer = render_worker.attach_writer(event_writer, config.client_write_timeout)?;
    client_writer.send_request(&ClientRequest::Detach).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
    for _ in 0..QUIET_OUTPUT_DRAIN_BATCH_LIMIT {
        async_pty_sender
            .send(SessionPaneOutputMessage::PaneExited)
            .await
            .map_err(|error| rootcause::report!("failed to queue muxr test pty event").attach(format!("{error}")))?;
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
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let session = self::run_test_client_session(
        &mut request_reader,
        &mut event_writer,
        &mut state,
        &mut async_pty_receiver,
        ClientSessionSelectBias::Output,
    );
    let client = async { self::recv_until_detached(&mut client_reader).await };

    let (session_result, client_result) = tokio::join!(session, client);

    session_result?;
    client_result?;
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(TrackedProcessState::Busy)
    );
    test_that::assert_that!(
        async_pty_receiver.try_recv(),
        err(matches_pattern!(TryRecvError::Empty))
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the test builds the queued PaneExited then PaneOutputReady boundary scenario end to end"
)]
async fn test_drain_queued_output_before_quiet_when_batch_limit_precedes_output_keeps_busy() -> rootcause::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
    Arc::make_mut(&mut config.user_config)
        .tracked_processes
        .push(self::tracked_cat_process("cl", Duration::from_secs(3)));
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
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Elapsed));
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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let _baseline_dirty_panes = runtimes.take_screen_dirty_panes();
    runtimes.handle(pane_id)?.write_input(b"muxr-queued-boundary\n")?;
    self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "muxr-queued-boundary")?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(PANE_OUTPUT_EVENT_CHANNEL_LIMIT);
    for _ in 0..QUIET_OUTPUT_DRAIN_BATCH_LIMIT {
        async_pty_sender
            .send(SessionPaneOutputMessage::PaneExited)
            .await
            .map_err(|error| rootcause::report!("failed to queue muxr test pty event").attach(format!("{error}")))?;
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
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut render_dmg = ClientRenderDmg::Clean;

    test_that::assert_that!(
        self::drain_queued_output_before_quiet(
            &mut async_pty_receiver,
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut render_dmg,
        )
        .await?,
        eq(OutputDrain::BatchLimitReached)
    );
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Elapsed));

    test_that::assert_that!(
        self::drain_queued_output_before_quiet(
            &mut async_pty_receiver,
            &mut event_writer,
            &mut state,
            &mut timers,
            &mut render_dmg,
        )
        .await?,
        eq(OutputDrain::Output)
    );

    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(TrackedProcessState::Busy)
    );
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Pending));
    test_that::assert_that!(
        async_pty_receiver.try_recv(),
        err(matches_pattern!(TryRecvError::Empty))
    );
    self::abort_client_drain(client_drain).await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn test_handle_pane_output_message_when_output_arrives_after_quiet_deadline_keeps_busy() -> rootcause::Result<()>
{
    let tempdir = tempfile::tempdir()?;
    let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
    Arc::make_mut(&mut config.user_config)
        .tracked_processes
        .push(self::tracked_cat_process("ct", Duration::from_secs(3)));
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
        &self::fg_tracked_process("cursor-agent"),
        then,
    );
    let mut timers = ClientTimers::new(&config)?;
    timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Elapsed));

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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let _baseline_dirty_panes = runtimes.take_screen_dirty_panes();
    runtimes.handle(pane_id)?.write_input(b"muxr-boundary\n")?;
    self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "muxr-boundary")?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut render_dmg = ClientRenderDmg::Clean;

    let keep_attached = crate::pty_output::handle_pane_output_message(
        Some(SessionPaneOutputMessage::PaneOutputReady),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    self::assert_output_sample_defers_runtime_process_discovery(
        &mut timers,
        &mut event_writer,
        &mut state,
        &mut render_dmg,
        pane_id,
    )
    .await?;
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(TrackedProcessState::Busy)
    );
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Pending));
    test_that::assert_that!(
        state.pane_tracked_processes.mark_quiet_deadlines(
            state.layout,
            self::instant_after(Instant::now(), Duration::from_secs(4))?
        )?,
        eq(TrackedProcessAttention::Seen)
    );
    self::abort_client_drain(client_drain).await;
    Ok(())
}

async fn assert_output_sample_defers_runtime_process_discovery(
    timers: &mut ClientTimers,
    event_writer: &mut impl ServerEventSink,
    state: &mut ClientSessionState<'_>,
    render_dmg: &mut ClientRenderDmg,
    pane_id: PaneId,
) -> rootcause::Result<()> {
    test_that::assert_that!(
        self::tracked_process_snapshot_label(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq("cu")
    );
    tokio::time::advance(Duration::from_millis(500)).await;
    test_that::assert_that!(
        crate::screen_render::handle_output_activity_sample(timers, event_writer, state, render_dmg).await?,
        eq(ClientSessionFlow::Continue)
    );
    test_that::assert_that!(
        self::tracked_process_snapshot_label(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq("ct")
    );
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
        .push(self::tracked_cat_process("cl", Duration::from_millis(30)));
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
    assert_mode(runtimes.handle(pane_id)?.application_mode())?;
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
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Elapsed));
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
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
    let (_request_reader, event_writer) = server_connection.split();
    let mut event_writer = render_worker.attach_writer(event_writer, config.client_write_timeout)?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut heartbeat_started_at = None;
    let mut render_dmg = ClientRenderDmg::Clean;

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
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), pane_id)?,
        eq(TrackedProcessState::Busy)
    );
    test_that::assert_that!(timers.tracked_process_quiet_deadline(), eq(QuietDeadline::Pending));
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
    test_that::assert_that!(layout.active_pane_id()?, eq(tracked_pane_id));
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(
        config.user_config.as_ref(),
        tracked_pane_id,
        &self::fg_tracked_process("cursor-agent"),
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
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut heartbeat_started_at = None;
    let mut render_dmg = ClientRenderDmg::Clean;

    let keep_attached = crate::request_router::handle_client_message(
        SessionClientMessage::Request(request),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut heartbeat_started_at,
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(state.layout.active_pane_id()?, eq(expected_active_pane));
    test_that::assert_that!(
        timers.tracked_process_quiet_sleep.deadline() < focused_deadline,
        eq(true)
    );
    self::abort_client_drain(client_drain).await;
    Ok(())
}

#[tokio::test]
async fn test_open_file_request_when_render_writer_is_closed_in_new_split_returns_disconnect() -> rootcause::Result<()>
{
    let tempdir = tempfile::tempdir()?;
    let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
    config.shell_cmd = crate::server::test_helpers::shell_cmd("/bin/cat");
    let layout = SessionLayout::initial(&config.session, self::metadata("cat", 1))?;
    test_that::assert_that!(
        self::open_file_request_with_closed_writer(tempdir, config, layout).await?,
        eq(ClientSessionFlow::Disconnect)
    );
    Ok(())
}

async fn open_file_request_with_closed_writer(
    _tempdir: tempfile::TempDir,
    config: ServerConfig,
    mut layout: SessionLayout,
) -> rootcause::Result<ClientSessionFlow> {
    crate::session::files::prepare_session_dirs(&config.paths)?;
    let terminal_size = TerminalSize::new(80, 24)?;
    let source_pane_id = PaneId::new(1)?;
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
    let pane_tracked_processes = PaneTrackedProcesses::default();
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    self::abort_client_drain(client_drain).await;
    render_worker.shutdown().await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut timers = ClientTimers::new(&config)?;
    let mut heartbeat_started_at = None;
    let mut render_dmg = ClientRenderDmg::Clean;

    crate::request_router::handle_client_message(
        SessionClientMessage::Request(ClientRequest::OpenFile {
            pane_id: source_pane_id,
            path: "/tmp/closed-writer.rs".to_owned(),
            line: None,
            column: None,
        }),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut heartbeat_started_at,
        &mut render_dmg,
    )
    .await
}

#[tokio::test]
async fn test_open_file_request_without_nvim_creates_vertical_split_and_writes_nvim_command() -> rootcause::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path().join("new-nvim-route.rs");
    self::open_file_request_without_nvim(tempdir, path, Some(42), Some(7), " '+call cursor(42,7)'").await
}

#[tokio::test]
async fn test_open_file_request_without_nvim_opens_directory_in_new_split() -> rootcause::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path().join("new-nvim-directory");
    std::fs::create_dir(&path)?;
    self::open_file_request_without_nvim(tempdir, path, None, None, "").await
}

async fn open_file_request_without_nvim(
    tempdir: tempfile::TempDir,
    path: std::path::PathBuf,
    line: Option<u32>,
    column: Option<u32>,
    expected_location: &str,
) -> rootcause::Result<()> {
    let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
    config.shell_cmd = crate::server::test_helpers::shell_cmd("/bin/cat");
    crate::session::files::prepare_session_dirs(&config.paths)?;
    let terminal_size = TerminalSize::new(80, 24)?;
    let mut layout = SessionLayout::initial(&config.session, self::metadata("cat", 1))?;
    let source_pane_id = PaneId::new(1)?;
    let new_pane_id = PaneId::new(2)?;
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
    let pane_tracked_processes = PaneTrackedProcesses::default();
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut timers = ClientTimers::new(&config)?;
    let mut heartbeat_started_at = None;
    let mut render_dmg = ClientRenderDmg::Clean;

    let keep_attached = crate::request_router::handle_client_message(
        SessionClientMessage::Request(ClientRequest::OpenFile {
            pane_id: source_pane_id,
            path: path
                .to_str()
                .ok_or_else(|| rootcause::report!("temporary test file path is not UTF-8"))?
                .to_owned(),
            line,
            column,
        }),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut heartbeat_started_at,
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(state.layout.active_pane_id()?, eq(new_pane_id));
    test_that::assert_that!(state.layout.active_tab()?.pane_ids().len(), eq(2));
    let expected_command = format!("nvim{expected_location} -- {}", self::utf8_path(&path)?);
    self::wait_for_runtime_snapshot_contains(&runtimes, new_pane_id, expected_command.trim_end())?;
    self::abort_client_drain(client_drain).await;
    Ok(())
}

#[tokio::test]
async fn test_open_file_request_when_right_pane_is_not_nvim_splits_right() -> rootcause::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
    config.shell_cmd = crate::server::test_helpers::shell_cmd("/bin/cat");
    crate::session::files::prepare_session_dirs(&config.paths)?;
    let terminal_size = TerminalSize::new(80, 24)?;
    let mut layout = self::layout(&config)?;
    let source_pane_id = PaneId::new(1)?;
    let right_pane_id = PaneId::new(2)?;
    layout.active_tab_mut()?.focus_pane(source_pane_id)?;
    let _unrelated_pane_id = layout.split_active_pane(
        config.user_config.layout,
        self::metadata("cat", 3),
        crate::pane::split::PaneSplitAxis::Horizontal,
    )?;
    layout.active_tab_mut()?.focus_pane(source_pane_id)?;
    let new_pane_id = PaneId::new(4)?;
    let path = tempdir.path().join("right-pane.rs");
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
    let pane_tracked_processes = PaneTrackedProcesses::default();
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    let mut timers = ClientTimers::new(&config)?;
    let mut heartbeat_started_at = None;
    let mut render_dmg = ClientRenderDmg::Clean;

    let keep_attached = crate::request_router::handle_client_message(
        SessionClientMessage::Request(ClientRequest::OpenFile {
            pane_id: source_pane_id,
            path: path
                .to_str()
                .ok_or_else(|| rootcause::report!("temporary test file path is not UTF-8"))?
                .to_owned(),
            line: None,
            column: None,
        }),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut heartbeat_started_at,
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(state.layout.active_pane_id()?, eq(new_pane_id));
    test_that::assert_that!(state.layout.active_tab()?.pane_ids().len(), eq(4));
    let right_pane_snapshot = PaneCmdSnapshot::try_from(&runtimes.handle(right_pane_id)?)?;
    test_that::assert_that!(
        PaneCmdObservation::from(&right_pane_snapshot).nvim_state(),
        eq(NvimState::NotRunning)
    );
    let expected_command = format!("nvim -- {}", self::utf8_path(&path)?);
    self::wait_for_runtime_snapshot_contains(&runtimes, new_pane_id, expected_command.trim_end())?;
    self::abort_client_drain(client_drain).await;
    Ok(())
}

#[tokio::test]
async fn test_open_file_request_when_source_is_fullscreen_preserves_hidden_sibling_and_splits_source()
-> rootcause::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
    config.shell_cmd = crate::server::test_helpers::shell_cmd("/bin/cat");
    crate::session::files::prepare_session_dirs(&config.paths)?;
    let terminal_size = TerminalSize::new(80, 24)?;
    let mut layout = self::layout(&config)?;
    let source_pane_id = PaneId::new(1)?;
    let editor_pane_id = PaneId::new(2)?;
    layout.active_tab_mut()?.focus_pane(source_pane_id)?;
    let path = tempdir.path().join("fullscreen-route.rs");
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
    let pane_tracked_processes = PaneTrackedProcesses::default();
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &config,
        &mut layout,
        &runtimes,
        &pane_tracked_processes,
        &terminal_size,
    )?;
    let (mut event_writer, client_drain) = self::connect_client_event_drain(&config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size,
    };
    crate::pane::fullscreen::handle_toggle_active_pane_cmd_client(&mut state)?;
    test_that::assert_that!(
        state.pane_fullscreen.visible_pane_id(state.layout)?,
        eq(Some(source_pane_id))
    );
    let mut timers = ClientTimers::new(&config)?;
    let mut heartbeat_started_at = None;
    let mut render_dmg = ClientRenderDmg::Clean;
    let path = path
        .to_str()
        .ok_or_else(|| rootcause::report!("temporary test file path is not UTF-8"))?;

    let keep_attached = crate::request_router::handle_client_message(
        SessionClientMessage::Request(ClientRequest::OpenFile {
            pane_id: source_pane_id,
            path: path.to_owned(),
            line: None,
            column: None,
        }),
        &mut event_writer,
        &mut state,
        &mut timers,
        &mut heartbeat_started_at,
        &mut render_dmg,
    )
    .await?;

    test_that::assert_that!(keep_attached, eq(ClientSessionFlow::Continue));
    test_that::assert_that!(state.pane_fullscreen.visible_pane_id(state.layout)?, none());
    test_that::assert_that!(state.layout.active_tab()?.pane_ids().len(), eq(3));
    let new_pane_id = state.layout.active_pane_id()?;
    test_that::assert_that!(new_pane_id, not(eq(source_pane_id)));
    test_that::assert_that!(new_pane_id, not(eq(editor_pane_id)));
    let expected_command = format!("nvim -- {path}");
    self::wait_for_runtime_snapshot_contains(&runtimes, new_pane_id, expected_command.trim_end())?;
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

fn tracked_cat_process(label: &'static str, quiet_threshold: Duration) -> TrackedProcess {
    TrackedProcess {
        id: TrackedProcessId::Claude,
        label,
        matchers: vec![ProcessMatcher::ExactExecutable("cat")],
        quiet_threshold,
        screen_observation: None,
    }
}

struct TrackedCatRuntimeFixture {
    _tempdir: tempfile::TempDir,
    config: ServerConfig,
    terminal_size: TerminalSize,
    layout: SessionLayout,
    pane_id: PaneId,
    runtimes: PaneRuntimes,
}

impl TrackedCatRuntimeFixture {
    fn write_screen_text(&self, text: &str) -> rootcause::Result<()> {
        // Move old output beyond the observed tail before writing the requested status rows.
        let output = format!("{}{text}\n", "\n".repeat(24));
        self.runtimes.handle(self.pane_id)?.write_input(output.as_bytes())?;
        let last_line = text
            .lines()
            .next_back()
            .ok_or_else(|| rootcause::report!("expected status text"))?;
        self::wait_for_runtime_snapshot_contains(&self.runtimes, self.pane_id, last_line)
    }
}

fn tracked_cat_runtime_fixture() -> rootcause::Result<TrackedCatRuntimeFixture> {
    let tempdir = tempfile::tempdir()?;
    let mut config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
    Arc::make_mut(&mut config.user_config)
        .tracked_processes
        .push(TrackedProcess {
            id: TrackedProcessId::Codex,
            label: "cx",
            matchers: vec![ProcessMatcher::ExactExecutable("cat")],
            quiet_threshold: Duration::from_secs(3),
            screen_observation: MuxrConfig::new()?
                .tracked_process_for_cmd("codex", None)
                .and_then(|process| process.screen_observation.clone()),
        });
    crate::session::files::prepare_session_dirs(&config.paths)?;
    let terminal_size = TerminalSize::new(80, 24)?;
    let mut layout = self::layout(&config)?;
    let pane_id = PaneId::new(1)?;
    layout.active_tab_mut()?.focus_pane(pane_id)?;
    let runtimes = PaneRuntimes::spawn_for_start_seed(
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
    Ok(TrackedCatRuntimeFixture {
        _tempdir: tempdir,
        config,
        terminal_size,
        layout,
        pane_id,
        runtimes,
    })
}

fn utf8_path(path: &std::path::Path) -> rootcause::Result<&str> {
    path.to_str()
        .ok_or_else(|| rootcause::report!("muxr test path is not UTF-8"))
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
        .ok_or_else(|| rootcause::report!("muxr test pane region is missing").attach(format!("pane_id={pane_id}")))?;
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
    PaneCmdObservation::FgCmd(crate::pane::cmd::FgCmd::from_test_cmd(PaneCmd {
        executable: executable.to_owned(),
        path: None,
        pid: 42,
    }))
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
            Some(ServerEvent::Pong) => {
                pong = true;
            }
            Some(ServerEvent::SidebarLayout(layout_snapshot)) => {
                test_that::assert_that!(
                    self::tracked_process_state(&layout_snapshot, pane_id)?,
                    eq(expected_state)
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
    render_worker: &mut RenderWorker,
) -> rootcause::Result<(crate::render_worker::RenderWorkerSender, tokio::task::JoinHandle<()>)> {
    let listener = ServerListener::bind(&config.paths.socket)?;
    let (client_connection, server_connection) =
        tokio::try_join!(ClientConnection::connect(&config.paths.socket), listener.accept())?;
    let (mut client_reader, _client_writer) = client_connection.split();
    let client_drain = tokio::spawn(async move { while let Ok(Some(_event)) = client_reader.recv_event().await {} });
    let (_request_reader, event_writer) = server_connection.split();
    Ok((
        render_worker.attach_writer(event_writer, config.client_write_timeout)?,
        client_drain,
    ))
}

async fn abort_client_drain(handle: tokio::task::JoinHandle<()>) {
    handle.abort();
    let _ = tokio::time::timeout(Duration::from_secs(1), handle).await;
}

fn tracked_process_state(layout_snapshot: &LayoutSnapshot, pane_id: PaneId) -> rootcause::Result<TrackedProcessState> {
    layout_snapshot
        .tabs()
        .iter()
        .flat_map(muxr_core::TabSnapshot::panes)
        .find(|pane| pane.id == pane_id)
        .map(|pane| pane.tracked_process_state)
        .ok_or_else(|| rootcause::report!("expected muxr pane snapshot").attach(format!("pane_id={pane_id}")))
}

fn tracked_process_snapshot_state(
    snapshot: &PaneTrackedProcessSnapshot,
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

fn tracked_process_snapshot_label(snapshot: &PaneTrackedProcessSnapshot, pane_id: PaneId) -> rootcause::Result<&str> {
    snapshot
        .panes()
        .find(|(snapshot_pane_id, _pane)| *snapshot_pane_id == pane_id)
        .map(|(_pane_id, pane)| pane.label())
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
    while runtimes.handle(pane_id)?.exit_state() == crate::pty::PtyExitState::Running {
        if started_at.elapsed() > Duration::from_secs(2) {
            return Err(
                rootcause::report!("timed out waiting for muxr test pane exit").attach(format!("pane_id={pane_id}"))
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn wait_for_runtime_fg_cmd(runtimes: &PaneRuntimes, pane_id: PaneId, expected: &str) -> rootcause::Result<()> {
    let started_at = Instant::now();
    loop {
        let handle = runtimes.handle(pane_id)?;
        let output_generation = handle.output_generation();
        let snapshot = PaneCmdSnapshot::try_from(&handle)?;
        if let PaneCmdObservation::FgCmd(fg_cmd) = PaneCmdObservation::from(&snapshot)
            && fg_cmd.leader_cmd().is_some_and(|cmd| cmd.executable == expected)
        {
            return Ok(());
        }
        let remaining = TEST_RUNTIME_READY_TIMEOUT.saturating_sub(started_at.elapsed());
        if remaining.is_zero() {
            return Err(
                rootcause::report!("timed out waiting for muxr runtime fg cmd").attach(format!("expected={expected}"))
            );
        }
        handle.wait_for_output(output_generation, remaining);
    }
}

fn wait_for_runtime_snapshot_contains(runtimes: &PaneRuntimes, pane_id: PaneId, needle: &str) -> rootcause::Result<()> {
    let started_at = Instant::now();
    loop {
        let handle = runtimes.handle(pane_id)?;
        let output_generation = handle.output_generation();
        let snapshot = handle.pane_render_snapshot(crate::terminal::TerminalSnapshotScope::Full)?;
        if self::snapshot_text(snapshot.terminal()).contains(needle) {
            return Ok(());
        }
        let remaining = TEST_RUNTIME_READY_TIMEOUT.saturating_sub(started_at.elapsed());
        if remaining.is_zero() {
            return Err(
                rootcause::report!("timed out waiting for muxr runtime snapshot").attach(format!("needle={needle}"))
            );
        }
        handle.wait_for_output(output_generation, remaining);
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
