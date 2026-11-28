use lazy_regex::Regex;
use muxr_config::ObservationPatterns;

use crate::client::session::tests::*;
use crate::pane::tracked_process::TrackedProcessDeadlineChange;

#[test]
fn test_tracked_process_when_codex_finishes_in_hidden_tab_raises_attention_after_quiet() -> rootcause::Result<()> {
    let mut fixture = tracked_cat_runtime_fixture()?;
    let handle = fixture.runtimes.handle(fixture.pane_id)?;
    handle.resize(&TerminalSize::new(80, 58)?)?;
    handle.write_input("\x1b[57;1HWorking (1s • esc to interrupt)\n".as_bytes())?;
    wait_for_runtime_snapshot_contains(&fixture.runtimes, fixture.pane_id, "to interrupt)")?;
    let _damage = fixture.runtimes.take_screen_dirty_panes();
    let mut processes = PaneTrackedProcesses::default();
    let started = Instant::now();
    processes.observe_runtime_pane_cmds(
        &fixture.config.user_config,
        &fixture.runtimes,
        &[fixture.pane_id],
        started,
    )?;
    fixture.layout.create_tab(metadata("sh", 3))?;

    let working_due = instant_after(started, Duration::from_secs(3))?;
    processes.guard_quiet_deadlines(
        &fixture.config.user_config,
        &fixture.layout,
        &fixture.runtimes,
        working_due,
    )?;
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, working_due)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Busy));

    handle.write_input("\x1b[57;1H\x1b[2K\x1b[15;1HSomething.\x1b[17;1HWorked for 24s • 20:42\n".as_bytes())?;
    wait_for_runtime_snapshot_contains(&fixture.runtimes, fixture.pane_id, "Worked for 24s")?;
    let dmg = fixture.runtimes.take_screen_dirty_panes();
    let finished = instant_after(working_due, Duration::from_secs(1))?;
    processes.record_screen_dmg(&fixture.runtimes, &dmg, finished)?;
    let due = instant_after(finished, Duration::from_secs(3))?;
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(Some(due)));
    processes.guard_quiet_deadlines(&fixture.config.user_config, &fixture.layout, &fixture.runtimes, due)?;

    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, due)?,
        eq(TrackedProcessAttention::Unseen {
            pane_ids: vec![fixture.pane_id]
        })
    );
    fixture.layout.focus_tab_at(0)?;
    processes.acknowledge_active_pane_attention(
        &fixture.config.user_config,
        &fixture.layout,
        &fixture.runtimes,
        due,
    )?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Seen));

    // Idle Codex emits the same cursor redraw on focus gain and loss. EOF flushes cat without adding a newline.
    for tab_index in [0, 1] {
        fixture.layout.focus_tab_at(tab_index)?;
        let generation = handle.output_generation();
        handle.write_input(b"\x1b[?2026h\x1b[0m\x1b[20;3H\x1b[?25h\x1b[?2026l\x04")?;
        handle.wait_for_output(generation, TEST_RUNTIME_READY_TIMEOUT);
        let dmg = fixture.runtimes.take_screen_dirty_panes();
        test_that::assert_that!(dmg.contains(&fixture.pane_id), eq(true));
        processes.record_screen_dmg(&fixture.runtimes, &dmg, due)?;

        test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Seen));
        test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(None));
    }

    handle.write_input(b"\x1b[?2026h\x1b[0m\x1b[20;3Hreply continues\x1b[?25h\x1b[?2026l\x04")?;
    wait_for_runtime_snapshot_contains(&fixture.runtimes, fixture.pane_id, "reply continues")?;
    let dmg = fixture.runtimes.take_screen_dirty_panes();
    processes.record_screen_dmg(&fixture.runtimes, &dmg, due)?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Busy));
    Ok(())
}

#[rstest::rstest]
#[case(false, false, false)]
#[case(false, true, false)]
#[case(true, false, false)]
#[case(true, true, false)]
#[case(false, false, true)]
#[case(true, true, true)]
#[tokio::test]
async fn test_codex_when_cancelled_clears_indicator_without_attention(
    #[case] focused: bool,
    #[case] timer_sample: bool,
    #[case] hard_wrapped: bool,
) -> rootcause::Result<()> {
    let mut fixture = crate::client::session::tests::tracked_process::tracked_cat_runtime_fixture()?;
    if !focused {
        fixture.layout.active_tab_mut()?.focus_pane(PaneId::new(2)?)?;
    }
    fixture.write_screen_text("Working (1s • esc to interrupt)")?;
    let now = Instant::now();
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], now)?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Busy));
    fixture.write_screen_text(if hard_wrapped {
        "■ Conversation interrupted - use\n/feedback if something went wrong"
    } else {
        "■ Conversation interrupted - use /feedback if something went wrong"
    })?;
    let due = crate::client::session::tests::tracked_process::instant_after(now, Duration::from_secs(3))?;
    let changes = if timer_sample {
        let _damage = fixture.runtimes.take_screen_dirty_panes();
        processes.guard_quiet_deadlines(&fixture.config.user_config, &fixture.layout, &fixture.runtimes, due)?
    } else {
        processes.record_screen_dmg(&fixture.runtimes, &[fixture.pane_id], now)?
    };
    // Nonempty effects have private constructors; compare their complete public state/deadline projection.
    test_that::assert_that!(
        (changes.state_change(), changes.deadline_change()),
        eq((
            TrackedProcessStateChange::Changed,
            TrackedProcessDeadlineChange::Changed
        ))
    );
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Seen));
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(None));
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, due)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    test_that::assert_that!(processes.attention_pane_ids(&fixture.layout), eq(Vec::new()));
    Ok(())
}

#[rstest::rstest]
#[case(
    "■ Conversation interrupted - use /feedback if something went wrong",
    TrackedProcessState::Seen
)]
#[case("streaming reply", TrackedProcessState::Busy)]
fn test_codex_when_discovery_precedes_pending_dmg_applies_each_frame_once(
    #[case] screen: &str,
    #[case] expected: TrackedProcessState,
) -> rootcause::Result<()> {
    let fixture = tracked_cat_runtime_fixture()?;
    let pane = fixture.pane_id;
    let mut processes = PaneTrackedProcesses::default();
    let now = Instant::now();
    fixture.write_screen_text(screen)?;
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[pane], now)?;
    let dmg = fixture.runtimes.take_screen_dirty_panes();
    test_that::assert_that!(dmg.contains(&pane), eq(true));
    processes.record_screen_dmg(&fixture.runtimes, &dmg, now)?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(expected));

    fixture.write_screen_text("next turn")?;
    processes.record_screen_dmg(&fixture.runtimes, &[pane], now)?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Busy));
    Ok(())
}

#[test]
fn test_codex_when_only_mouse_reporting_changes_refreshes_mode_and_keeps_dot_neutral() -> rootcause::Result<()> {
    let fixture = tracked_cat_runtime_fixture()?;
    let pane = fixture.pane_id;
    let handle = fixture.runtimes.handle(pane)?;
    let now = Instant::now();
    let mut processes = PaneTrackedProcesses::default();
    fixture.write_screen_text("idle prompt")?;
    let initial = fixture.runtimes.take_screen_dirty_panes();
    processes.record_screen_dmg(&fixture.runtimes, &initial, now)?;
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[pane], now)?;

    // EOF flushes cat's canonical input without printing a newline or moving the cursor.
    let set_mode = |input: &[u8], expected| -> rootcause::Result<()> {
        handle.write_input(input)?;
        let start = Instant::now();
        loop {
            let generation = handle.output_generation();
            if handle.mouse_mode() == expected {
                return Ok(());
            }
            let remaining = TEST_RUNTIME_READY_TIMEOUT.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Err(
                    rootcause::report!("timed out waiting for mouse mode").attach(format!("expected={expected:?}"))
                );
            }
            handle.wait_for_output(generation, remaining);
        }
    };
    set_mode(b"\x1b[?1000h\x04", muxr_core::PaneMouseMode::PressRelease)?;
    let dmg = fixture.runtimes.take_screen_dirty_panes();
    processes.record_screen_dmg(&fixture.runtimes, &dmg, now)?;

    test_that::assert_that!(dmg.contains(&pane), eq(true));
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Seen));
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(None));

    fixture.write_screen_text("reply")?;
    set_mode(b"\x1b[?1000l\x04", muxr_core::PaneMouseMode::None)?;
    let dmg = fixture.runtimes.take_screen_dirty_panes();
    processes.record_screen_dmg(&fixture.runtimes, &dmg, now)?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Busy));
    Ok(())
}

#[test]
fn test_codex_when_old_interruption_enters_tail_resumes_on_following_output() -> rootcause::Result<()> {
    let fixture = tracked_cat_runtime_fixture()?;
    let pane = fixture.pane_id;
    let handle = fixture.runtimes.handle(pane)?;
    handle.resize(&TerminalSize::new(80, 24)?)?;
    let banner = "■ Conversation interrupted - use /feedback if something went wrong";
    let now = Instant::now();
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[pane], now)?;

    for (frame, ready, expected) in [
        (
            format!("\x1b[12;1H{banner}\x1b[18;1Hreply\n"),
            "reply",
            TrackedProcessState::Busy,
        ),
        (
            "\x1b[12;1H\x1b[Lshifted\n".to_owned(),
            "shifted",
            TrackedProcessState::Seen,
        ),
        (
            "\x1b[19;1Hreply continues\n".to_owned(),
            "reply continues",
            TrackedProcessState::Busy,
        ),
        (
            format!("\x1b[20;1H{banner}\x1b[21;1Hstopped\n"),
            "stopped",
            TrackedProcessState::Seen,
        ),
    ] {
        handle.write_input(frame.as_bytes())?;
        wait_for_runtime_snapshot_contains(&fixture.runtimes, pane, ready)?;
        let dmg = fixture.runtimes.take_screen_dirty_panes();
        processes.record_screen_dmg(&fixture.runtimes, &dmg, now)?;

        test_that::assert_that!(fixture.tracked_state(&processes)?, eq(expected));
    }
    Ok(())
}

#[test]
fn test_codex_when_output_scrolls_past_interruption_rearms_until_next_interruption() -> rootcause::Result<()> {
    let fixture = tracked_cat_runtime_fixture()?;
    let pane = fixture.pane_id;
    let mut processes = PaneTrackedProcesses::default();
    let now = Instant::now();
    fixture.write_screen_text("Working (1s • esc to interrupt)")?;
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[pane], now)?;

    let interrupted = "■ Conversation interrupted - use /feedback if something went wrong";
    fixture.write_screen_text(interrupted)?;
    processes.record_screen_dmg(&fixture.runtimes, &[pane], now)?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Seen));

    let handle = fixture.runtimes.handle(pane)?;
    handle.write_input(b"reply continues\n")?;
    wait_for_runtime_snapshot_contains(&fixture.runtimes, pane, "reply continues")?;
    processes.record_screen_dmg(&fixture.runtimes, &[pane], now)?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Busy));

    handle.write_input(format!("{interrupted}\nsecond stop\n").as_bytes())?;
    wait_for_runtime_snapshot_contains(&fixture.runtimes, pane, "second stop")?;
    processes.record_screen_dmg(&fixture.runtimes, &[pane], now)?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Seen));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn test_handle_cmd_handoff_sample_when_output_sample_is_pending_removes_stale_sample() -> rootcause::Result<()> {
    let mut fixture = crate::client::session::tests::tracked_process::tracked_cat_runtime_fixture()?;
    let mut timers = ClientTimers::new(&fixture.config)?;
    timers.schedule_output_activity_samples(&[fixture.pane_id])?;
    timers.schedule_cmd_handoff_sample(fixture.pane_id)?;
    let pane_tracked_processes = PaneTrackedProcesses::default();
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &fixture.config,
        &mut fixture.layout,
        &fixture.runtimes,
        &pane_tracked_processes,
        &fixture.terminal_size,
    )?;
    let (mut event_writer, client_drain) =
        crate::client::session::tests::tracked_process::connect_client_event_drain(&fixture.config, &mut render_worker)
            .await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = crate::client::session::tests::tracked_process::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &fixture.config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut fixture.layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut fixture.runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size: fixture.terminal_size,
    };
    let mut render_dmg = ClientRenderDmg::Clean;

    test_that::assert_that!(
        crate::screen_render::handle_cmd_handoff_sample(&mut timers, &mut event_writer, &mut state, &mut render_dmg,)
            .await?,
        eq(ClientSessionFlow::Continue)
    );
    tokio::time::advance(Duration::from_millis(500)).await;
    test_that::assert_that!(timers.take_due_output_activity_sample_panes()?, eq(Vec::new()));

    crate::client::session::tests::tracked_process::abort_client_drain(client_drain).await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn test_handle_output_activity_sample_when_runtime_process_is_tracked_marks_busy() -> rootcause::Result<()> {
    let mut fixture = crate::client::session::tests::tracked_process::tracked_cat_runtime_fixture()?;
    fixture.write_screen_text("Working (1s • esc to interrupt)")?;
    let mut timers = ClientTimers::new(&fixture.config)?;
    timers.schedule_output_activity_samples(&[fixture.pane_id])?;
    let pane_tracked_processes = PaneTrackedProcesses::default();
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &fixture.config,
        &mut fixture.layout,
        &fixture.runtimes,
        &pane_tracked_processes,
        &fixture.terminal_size,
    )?;
    let (mut event_writer, client_drain) =
        crate::client::session::tests::tracked_process::connect_client_event_drain(&fixture.config, &mut render_worker)
            .await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = crate::client::session::tests::tracked_process::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &fixture.config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut fixture.layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut fixture.runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size: fixture.terminal_size,
    };
    let mut render_dmg = ClientRenderDmg::Clean;

    tokio::time::advance(Duration::from_millis(500)).await;
    test_that::assert_that!(
        crate::screen_render::handle_output_activity_sample(
            &mut timers,
            &mut event_writer,
            &mut state,
            &mut render_dmg,
        )
        .await?,
        eq(ClientSessionFlow::Continue)
    );
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&state.pane_tracked_processes.snapshot(state.layout), fixture.pane_id)?,
        eq(TrackedProcessState::Busy)
    );

    crate::client::session::tests::tracked_process::abort_client_drain(client_drain).await;
    Ok(())
}

#[rstest::rstest]
#[case(false, TrackedProcessState::Seen, false)]
#[case(true, TrackedProcessState::Seen, false)]
#[case(false, TrackedProcessState::Busy, false)]
#[case(true, TrackedProcessState::Busy, false)]
#[case(false, TrackedProcessState::Seen, true)]
#[case(true, TrackedProcessState::Seen, true)]
#[tokio::test]
async fn test_codex_when_enter_echoes_preserves_indicator(
    #[case] structured_key: bool,
    #[case] initial_state: TrackedProcessState,
    #[case] pending_reply: bool,
) -> rootcause::Result<()> {
    let mut fixture = crate::client::session::tests::tracked_process::tracked_cat_runtime_fixture()?;
    let completed = "  Worked for 16m 15s • 14:18";
    fixture.write_screen_text(if initial_state == TrackedProcessState::Busy {
        "Working (1s • esc to interrupt)"
    } else {
        completed
    })?;
    // Establish the idle/working screen before the next turn's output arrives.
    let _baseline = fixture.runtimes.take_screen_dirty_panes();
    let then = Instant::now();
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    pane_tracked_processes.observe_runtime_pane_cmds(
        &fixture.config.user_config,
        &fixture.runtimes,
        &[fixture.pane_id],
        then,
    )?;
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &fixture.config,
        &mut fixture.layout,
        &fixture.runtimes,
        &pane_tracked_processes,
        &fixture.terminal_size,
    )?;
    if pending_reply {
        fixture.write_screen_text("reply before the key")?;
    }
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = crate::client::session::tests::tracked_process::pty_event_channel();
    let mut sink_guards = Vec::new();
    let mut state = ClientSessionState {
        pane_tracked_processes,
        config: &fixture.config,
        delete_sessions: &delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot: layout_snapshot,
        layout: &mut fixture.layout,
        pane_fullscreen: PaneFullscreen::default(),
        pty_event_sender: &pty_event_sender,
        render_worker: &mut render_worker,
        runtimes: &mut fixture.runtimes,
        scrollback_editor: None,
        sink_guards: &mut sink_guards,
        terminal_size: fixture.terminal_size,
    };
    if structured_key {
        crate::pane::input::handle_client_key(
            &ClientKey {
                code: ClientKeyCode::Enter,
                modifiers: ClientKeyModifiers::NONE,
                raw_bytes: b"\r".to_vec(),
            },
            &mut state,
        )?;
    } else {
        crate::pane::input::handle_client_input(b"\r", &mut state)?;
    }
    let delivered_at = instant_after(Instant::now(), Duration::from_millis(600))?;
    let expected = if pending_reply {
        TrackedProcessState::Busy
    } else {
        initial_state
    };
    assert_codex_enter_echo(&mut state, fixture.pane_id, expected, delivered_at)
}

#[rstest::rstest]
#[case("Working (1s • esc to interrupt)", TrackedProcessState::Busy, true)]
#[case("Worked for 1s • 14:18", TrackedProcessState::Seen, false)]
#[case("ready for input", TrackedProcessState::Seen, false)]
fn test_codex_when_first_discovered_on_focus_seeds_visible_status(
    #[case] text: &str,
    #[case] expected: TrackedProcessState,
    #[case] has_deadline: bool,
) -> rootcause::Result<()> {
    let fixture = crate::client::session::tests::tracked_process::tracked_cat_runtime_fixture()?;
    fixture.write_screen_text(text)?;
    let now = Instant::now();
    let mut processes = PaneTrackedProcesses::default();
    processes.acknowledge_active_pane_attention(
        &fixture.config.user_config,
        &fixture.layout,
        &fixture.runtimes,
        now,
    )?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(expected));
    let expected_deadline = if has_deadline {
        Some(crate::client::session::tests::tracked_process::instant_after(
            now,
            Duration::from_secs(3),
        )?)
    } else {
        None
    };
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(expected_deadline));
    Ok(())
}

#[rstest::rstest]
#[case(false, TrackedProcessState::Seen)]
#[case(true, TrackedProcessState::Unseen)]
fn test_tracked_process_when_working_disappears_settles_based_on_focus(
    #[case] unfocused: bool,
    #[case] expected_state: TrackedProcessState,
) -> rootcause::Result<()> {
    let mut fixture = tracked_cat_runtime_fixture()?;
    if unfocused {
        fixture.layout.active_tab_mut()?.focus_pane(PaneId::new(2)?)?;
    }
    let mut processes = PaneTrackedProcesses::default();
    let started = Instant::now();
    fixture.write_screen_text("Worked for 8s • 20:58")?;
    processes.observe_runtime_pane_cmds(
        &fixture.config.user_config,
        &fixture.runtimes,
        &[fixture.pane_id],
        started,
    )?;
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(None));

    fixture.write_screen_text("Working (1s • esc to interrupt)")?;
    processes.record_screen_dmg(&fixture.runtimes, &[fixture.pane_id], started)?;
    let finished = instant_after(started, Duration::from_secs(1))?;
    fixture.write_screen_text("Worked for 8s • 20:58")?;
    let dmg = fixture.runtimes.take_screen_dirty_panes();
    processes.record_screen_dmg(&fixture.runtimes, &dmg, finished)?;
    let due = instant_after(finished, Duration::from_secs(3))?;
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(Some(due)));

    processes.guard_quiet_deadlines(&fixture.config.user_config, &fixture.layout, &fixture.runtimes, due)?;
    let attention = processes.mark_quiet_deadlines(&fixture.layout, due)?;

    let expected_attention = if unfocused {
        TrackedProcessAttention::Unseen {
            pane_ids: vec![fixture.pane_id],
        }
    } else {
        TrackedProcessAttention::Seen
    };
    test_that::assert_that!(attention, eq(expected_attention));
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(expected_state));
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(None));
    Ok(())
}

#[test]
fn test_tracked_process_when_damage_precedes_wakeup_keeps_busy_and_preserves_damage() -> rootcause::Result<()> {
    let mut fixture = tracked_cat_runtime_fixture()?;
    fixture.layout.active_tab_mut()?.focus_pane(PaneId::new(2)?)?;
    fixture.write_screen_text("Working (0s • esc to interrupt)")?;
    let started = Instant::now();
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(
        &fixture.config.user_config,
        &fixture.runtimes,
        &[fixture.pane_id],
        started,
    )?;
    let _damage = fixture.runtimes.take_screen_dirty_panes();
    fixture.write_screen_text("streamed output")?;

    // The terminal has changed, but no output wakeup has been handled.
    let due = instant_after(started, Duration::from_secs(3))?;
    processes.guard_quiet_deadlines(&fixture.config.user_config, &fixture.layout, &fixture.runtimes, due)?;

    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, due)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Busy));
    test_that::assert_that!(
        processes.next_quiet_deadline(&fixture.layout)?,
        eq(Some(instant_after(due, Duration::from_secs(3))?))
    );
    test_that::assert_that!(fixture.runtimes.take_screen_dirty_panes(), eq(vec![fixture.pane_id]));
    Ok(())
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
fn test_tracked_process_screen_when_quiet_deadline_is_not_due_preserves_deadline(
    #[case] recent_input: bool,
) -> rootcause::Result<()> {
    let fixture = crate::client::session::tests::tracked_process::tracked_cat_runtime_fixture()?;
    let handle = fixture.runtimes.handle(fixture.pane_id)?;
    let busy = format!("{}Working (6m 35s • ctrl+x to interrupt)\n", "\n".repeat(24));
    handle.write_input(busy.as_bytes())?;
    crate::client::session::tests::tracked_process::wait_for_runtime_snapshot_contains(
        &fixture.runtimes,
        fixture.pane_id,
        "to interrupt)",
    )?;
    let then = Instant::now();
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], then)?;
    let check_at = if recent_input {
        processes.record_user_interaction(
            &fixture.layout,
            fixture.pane_id,
            TrackedProcessUserInteraction::MayEcho,
            crate::client::session::tests::tracked_process::instant_after(then, Duration::from_secs(2))?,
        )?;
        crate::client::session::tests::tracked_process::instant_after(then, Duration::from_secs(3))?
    } else {
        crate::client::session::tests::tracked_process::instant_after(then, Duration::from_secs(1))?
    };
    let deadline = processes.next_quiet_deadline(&fixture.layout)?;
    test_that::assert_that!(
        processes.guard_quiet_deadlines(
            &fixture.config.user_config,
            &fixture.layout,
            &fixture.runtimes,
            check_at
        )?,
        eq(TrackedProcessChanges::default())
    );
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(deadline));
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, check_at)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    Ok(())
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
fn test_codex_when_output_resumes_without_banner_marks_settled_pane_busy(
    #[case] acknowledged: bool,
) -> rootcause::Result<()> {
    let mut fixture = crate::client::session::tests::tracked_process::tracked_cat_runtime_fixture()?;
    fixture.layout.active_tab_mut()?.focus_pane(PaneId::new(2)?)?;
    fixture.write_screen_text("Working (0s • esc to interrupt)")?;
    let then = Instant::now();
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], then)?;
    fixture.write_screen_text("Something.")?;
    let dmg = fixture.runtimes.take_screen_dirty_panes();
    processes.record_screen_dmg(&fixture.runtimes, &dmg, then)?;
    let due = instant_after(then, Duration::from_secs(3))?;
    processes.guard_quiet_deadlines(&fixture.config.user_config, &fixture.layout, &fixture.runtimes, due)?;
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, due)?,
        eq(TrackedProcessAttention::Unseen {
            pane_ids: vec![fixture.pane_id]
        })
    );
    if acknowledged {
        processes.acknowledge_attention(fixture.pane_id);
    }

    fixture.write_screen_text("Streaming again")?;
    let dmg = fixture.runtimes.take_screen_dirty_panes();
    processes.record_screen_dmg(&fixture.runtimes, &dmg, due)?;

    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Busy));
    test_that::assert_that!(
        processes.next_quiet_deadline(&fixture.layout)?,
        eq(Some(instant_after(due, Duration::from_secs(3))?))
    );
    Ok(())
}

#[test]
fn test_tracked_process_screen_when_foreground_is_untracked_discards_stale_busy_footer() -> rootcause::Result<()> {
    let fixture = crate::client::session::tests::tracked_process::tracked_cat_runtime_fixture()?;
    let handle = fixture.runtimes.handle(fixture.pane_id)?;
    let busy = format!("{}Working (6m 35s • ctrl+x to interrupt)\n", "\n".repeat(24));
    handle.write_input(busy.as_bytes())?;
    crate::client::session::tests::tracked_process::wait_for_runtime_snapshot_contains(
        &fixture.runtimes,
        fixture.pane_id,
        "to interrupt)",
    )?;
    let then = Instant::now();
    let now = crate::client::session::tests::tracked_process::instant_after(then, Duration::from_secs(3))?;
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], then)?;
    // The ordinary config does not track this fixture's cat process. Fresh process evidence must win over the
    // Working banner left in its terminal and the previously cached Codex identity.
    let changes = processes.guard_quiet_deadlines(&MuxrConfig::new()?, &fixture.layout, &fixture.runtimes, now)?;
    test_that::assert_that!(changes.state_change(), eq(TrackedProcessStateChange::Changed));
    test_that::assert_that!(
        processes.snapshot(&fixture.layout),
        eq(PaneTrackedProcessSnapshot::default())
    );
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(None));
    Ok(())
}

#[rstest::rstest]
#[case(TrackedProcessId::Codex)]
#[case(TrackedProcessId::Cursor)]
fn test_tracked_process_screen_when_config_overrides_patterns_and_trimming_drives_busy_and_attention(
    #[case] agent: TrackedProcessId,
) -> rootcause::Result<()> {
    let mut fixture = crate::client::session::tests::tracked_process::tracked_cat_runtime_fixture()?;
    fixture.layout.active_tab_mut()?.focus_pane(PaneId::new(2)?)?;
    let process = Arc::make_mut(&mut fixture.config.user_config)
        .tracked_processes
        .processes
        .iter_mut()
        .find(|process| process.matches("cat", None))
        .ok_or_else(|| rootcause::report!("missing configured cat process"))?;
    process.id = agent;
    process.screen_observation = Some(ScreenObservationConfig {
        busy: ObservationPatterns::try_new(vec![Regex::clone(lazy_regex::regex!(r"\ATASK\s+ACTIVE\z"))])?,
        needs_attention: Some(ObservationPatterns::try_new(vec![Regex::clone(lazy_regex::regex!(
            r"\ATASK\s+DONE\b"
        ))])?),
        cancelled: Some(ObservationPatterns::try_new(vec![Regex::clone(lazy_regex::regex!(
            r"\ATASK\s+CANCELLED\z"
        ))])?),
        trim_chars: &['#'],
    });
    let mut processes = PaneTrackedProcesses::default();
    let then = Instant::now();
    fixture.write_screen_text("# TASK ACTIVE #")?;
    processes.observe_runtime_pane_cmds(
        fixture.config.user_config.as_ref(),
        &fixture.runtimes,
        &[fixture.pane_id],
        then,
    )?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Busy));
    fixture.write_screen_text("# TASK output #")?;
    let dmg = fixture.runtimes.take_screen_dirty_panes();
    processes.record_screen_dmg(&fixture.runtimes, &dmg, then)?;
    let mut due = instant_after(then, Duration::from_secs(3))?;
    if agent == TrackedProcessId::Cursor {
        processes.guard_quiet_deadlines(
            fixture.config.user_config.as_ref(),
            &fixture.layout,
            &fixture.runtimes,
            due,
        )?;
        test_that::assert_that!(
            processes.mark_quiet_deadlines(&fixture.layout, due)?,
            eq(TrackedProcessAttention::Unchanged)
        );
        fixture.write_screen_text("# TASK DONE turn 1 #")?;
        due = instant_after(due, Duration::from_secs(3))?;
    }
    processes.guard_quiet_deadlines(
        fixture.config.user_config.as_ref(),
        &fixture.layout,
        &fixture.runtimes,
        due,
    )?;
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, due)?,
        eq(TrackedProcessAttention::Unseen {
            pane_ids: vec![fixture.pane_id]
        })
    );
    fixture.write_screen_text("# TASK ACTIVE #")?;
    processes.record_screen_dmg(&fixture.runtimes, &[fixture.pane_id], due)?;
    fixture.write_screen_text("# TASK CANCELLED #")?;
    processes.record_screen_dmg(&fixture.runtimes, &[fixture.pane_id], due)?;
    test_that::assert_that!(fixture.tracked_state(&processes)?, eq(TrackedProcessState::Seen));
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(None));
    test_that::assert_that!(processes.attention_pane_ids(&fixture.layout), eq(Vec::new()));
    Ok(())
}
