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
    test_that::assert_that!(
        tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(TrackedProcessState::Busy)
    );

    handle.write_input("\x1b[57;1H\x1b[2K\x1b[15;1HSomething.\x1b[17;1HWorked for 24s • 20:42\n".as_bytes())?;
    wait_for_runtime_snapshot_contains(&fixture.runtimes, fixture.pane_id, "Worked for 24s")?;
    let damage = fixture.runtimes.take_screen_dirty_panes();
    let finished = instant_after(working_due, Duration::from_secs(1))?;
    processes.record_cached_visible_activity(&fixture.runtimes, &damage, finished)?;
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
    test_that::assert_that!(
        tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(TrackedProcessState::Seen)
    );
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
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(TrackedProcessState::Busy)
    );
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
        processes.record_cached_visible_activity(&fixture.runtimes, &[fixture.pane_id], now)?
    };
    // Nonempty effects have private constructors; compare their complete public state/deadline projection.
    test_that::assert_that!(
        (changes.state_change(), changes.deadline_change()),
        eq((
            TrackedProcessStateChange::Changed,
            TrackedProcessDeadlineChange::Changed
        ))
    );
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(TrackedProcessState::Seen)
    );
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(None));
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, due)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    test_that::assert_that!(processes.attention_pane_ids(&fixture.layout), eq(Vec::new()));
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
#[case(false, TrackedProcessState::Seen)]
#[case(true, TrackedProcessState::Seen)]
#[case(false, TrackedProcessState::Busy)]
#[case(true, TrackedProcessState::Busy)]
#[tokio::test]
async fn test_codex_when_enter_leaves_banner_absent_does_not_start_work(
    #[case] structured_key: bool,
    #[case] initial_state: TrackedProcessState,
) -> rootcause::Result<()> {
    let mut fixture = crate::client::session::tests::tracked_process::tracked_cat_runtime_fixture()?;
    let completed = "  Worked for 16m 15s • 14:18";
    fixture.write_screen_text(if initial_state == TrackedProcessState::Busy {
        "Working (1s • esc to interrupt)"
    } else {
        completed
    })?;
    let then = Instant::now();
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    pane_tracked_processes.observe_runtime_pane_cmds(
        &fixture.config.user_config,
        &fixture.runtimes,
        &[fixture.pane_id],
        then,
    )?;
    let due = instant_after(then, Duration::from_secs(3))?;
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &fixture.config,
        &mut fixture.layout,
        &fixture.runtimes,
        &pane_tracked_processes,
        &fixture.terminal_size,
    )?;
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
    assert_codex_waits_for_working_after_enter(&mut state, fixture.pane_id, initial_state, due)
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
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(expected)
    );
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
    processes.record_cached_visible_activity(&fixture.runtimes, &[fixture.pane_id], started)?;
    let finished = instant_after(started, Duration::from_secs(1))?;
    fixture.write_screen_text("Worked for 8s • 20:58")?;
    let damage = fixture.runtimes.take_screen_dirty_panes();
    processes.record_cached_visible_activity(&fixture.runtimes, &damage, finished)?;
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
    test_that::assert_that!(
        tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(expected_state)
    );
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
    test_that::assert_that!(
        tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(TrackedProcessState::Busy)
    );
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
#[case(false, TrackedProcessState::Busy, "Working (6m 35s • ctrl+x to interrupt)")]
#[case(true, TrackedProcessState::Busy, "Working (6m 35s • ctrl+x to interrupt)")]
#[case(false, TrackedProcessState::Unseen, "Worked for 22m 38s • 11:37")]
#[case(true, TrackedProcessState::Seen, "Worked for 22m 38s • 11:37")]
fn test_tracked_process_screen_when_pane_is_settled_rearms_only_for_working(
    #[case] acknowledged: bool,
    #[case] expected_state: TrackedProcessState,
    #[case] text: &str,
) -> rootcause::Result<()> {
    let mut fixture = crate::client::session::tests::tracked_process::tracked_cat_runtime_fixture()?;
    fixture.layout.active_tab_mut()?.focus_pane(PaneId::new(2)?)?;
    fixture.write_screen_text("Working (0s • esc to interrupt)")?;
    let then = Instant::now();
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], then)?;
    fixture.write_screen_text("Something.")?;
    let damage = fixture.runtimes.take_screen_dirty_panes();
    processes.record_cached_visible_activity(&fixture.runtimes, &damage, then)?;
    let due = instant_after(then, Duration::from_secs(3))?;
    processes.guard_quiet_deadlines(&fixture.config.user_config, &fixture.layout, &fixture.runtimes, due)?;
    processes.mark_quiet_deadlines(&fixture.layout, due)?;
    if acknowledged {
        processes.acknowledge_attention(fixture.pane_id);
    }

    fixture.write_screen_text("redraw")?;
    let handle = fixture.runtimes.handle(fixture.pane_id)?;
    let output = format!("{}{text}\n", "\n".repeat(24));
    handle.write_input(output.as_bytes())?;
    crate::client::session::tests::tracked_process::wait_for_runtime_snapshot_contains(
        &fixture.runtimes,
        fixture.pane_id,
        text,
    )?;
    let now = crate::client::session::tests::tracked_process::instant_after(then, Duration::from_secs(30))?;
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], now)?;
    processes.record_cached_visible_activity(&fixture.runtimes, &[fixture.pane_id], now)?;
    test_that::assert_that!(
        processes.guard_quiet_deadlines(&fixture.config.user_config, &fixture.layout, &fixture.runtimes, now)?,
        eq(TrackedProcessChanges::default())
    );
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, now)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(expected_state)
    );
    let expected_deadline = if expected_state == TrackedProcessState::Busy {
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
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(TrackedProcessState::Busy)
    );
    fixture.write_screen_text("# TASK output #")?;
    let damage = fixture.runtimes.take_screen_dirty_panes();
    processes.record_cached_visible_activity(&fixture.runtimes, &damage, then)?;
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
    processes.record_cached_visible_activity(&fixture.runtimes, &[fixture.pane_id], due)?;
    fixture.write_screen_text("# TASK CANCELLED #")?;
    processes.record_cached_visible_activity(&fixture.runtimes, &[fixture.pane_id], due)?;
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(TrackedProcessState::Seen)
    );
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(None));
    test_that::assert_that!(processes.attention_pane_ids(&fixture.layout), eq(Vec::new()));
    Ok(())
}
