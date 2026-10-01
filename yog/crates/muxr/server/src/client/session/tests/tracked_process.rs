use muxr_config::ObservationPatterns;

use super::*;

#[tokio::test(start_paused = true)]
async fn test_handle_cmd_handoff_sample_when_output_sample_is_pending_removes_stale_sample() -> rootcause::Result<()> {
    let mut fixture = self::tracked_cat_runtime_fixture()?;
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
        self::connect_client_event_drain(&fixture.config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
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

    self::abort_client_drain(client_drain).await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn test_handle_output_activity_sample_when_runtime_process_is_tracked_marks_busy() -> rootcause::Result<()> {
    let mut fixture = self::tracked_cat_runtime_fixture()?;
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
        self::connect_client_event_drain(&fixture.config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
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

    self::abort_client_drain(client_drain).await;
    Ok(())
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn test_prompt_submit_when_old_completion_remains_waits_for_fresh_completion(
    #[case] structured_key: bool,
) -> rootcause::Result<()> {
    let mut fixture = self::tracked_cat_runtime_fixture()?;
    fixture.write_screen_text("  Worked for 16m 15s • 14:18")?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let then = Instant::now();
    pane_tracked_processes.observe_runtime_pane_cmds(
        &fixture.config.user_config,
        &fixture.runtimes,
        &[fixture.pane_id],
        then,
    )?;
    let due = self::instant_after(then, Duration::from_secs(3))?;
    pane_tracked_processes.guard_quiet_deadlines(
        &fixture.config.user_config,
        &fixture.layout,
        &fixture.runtimes,
        due,
    )?;
    pane_tracked_processes.mark_quiet_deadlines(&fixture.layout, due)?;
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &fixture.config,
        &mut fixture.layout,
        &fixture.runtimes,
        &pane_tracked_processes,
        &fixture.terminal_size,
    )?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
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
    self::assert_submission_waits_for_fresh_completion(&mut state, fixture.pane_id)
}

#[rstest::rstest]
#[case(false, TrackedProcessState::Seen)]
#[case(true, TrackedProcessState::Seen)]
#[case(false, TrackedProcessState::Busy)]
#[case(true, TrackedProcessState::Busy)]
#[tokio::test]
async fn test_codex_when_enter_does_not_start_work_preserves_completion(
    #[case] structured_key: bool,
    #[case] initial_state: TrackedProcessState,
) -> rootcause::Result<()> {
    let mut fixture = self::tracked_cat_runtime_fixture()?;
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
    let due = self::instant_after(then, Duration::from_secs(3))?;
    if initial_state == TrackedProcessState::Seen {
        pane_tracked_processes.guard_quiet_deadlines(
            &fixture.config.user_config,
            &fixture.layout,
            &fixture.runtimes,
            due,
        )?;
        pane_tracked_processes.mark_quiet_deadlines(&fixture.layout, due)?;
    } else {
        fixture.write_screen_text(completed)?;
    }
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &fixture.config,
        &mut fixture.layout,
        &fixture.runtimes,
        &pane_tracked_processes,
        &fixture.terminal_size,
    )?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
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
    self::assert_ignored_enter_preserves_completion(&mut state, fixture.pane_id, initial_state, due)
}

#[rstest::rstest]
#[case(TrackedProcessState::Seen)]
#[case(TrackedProcessState::Unseen)]
#[tokio::test]
async fn test_handle_pane_output_message_when_completion_is_repainted_allows_quiet(
    #[case] expected_state: TrackedProcessState,
) -> rootcause::Result<()> {
    let mut fixture = self::tracked_cat_runtime_fixture()?;
    if expected_state == TrackedProcessState::Unseen {
        fixture.layout.active_tab_mut()?.focus_pane(PaneId::new(2)?)?;
    }
    let completed = "  Worked for 16m 15s • 14:18";
    fixture.write_screen_text(completed)?;
    let before = fixture.runtimes.handle(fixture.pane_id)?.live_tail_text(12);
    let then = Instant::now()
        .checked_sub(Duration::from_secs(4))
        .ok_or_else(|| rootcause::report!("test instant underflowed"))?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    pane_tracked_processes.observe_runtime_pane_cmds(
        &fixture.config.user_config,
        &fixture.runtimes,
        &[fixture.pane_id],
        then,
    )?;
    pane_tracked_processes.record_user_interaction(
        &fixture.layout,
        fixture.pane_id,
        TrackedProcessUserInteraction::StartsTrackedProcessWork,
        then,
    )?;
    let mut timers = ClientTimers::new(&fixture.config)?;
    timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &fixture.layout)?;
    let (layout_snapshot, mut render_worker) = crate::screen_render::initial_client_render(
        &fixture.config,
        &mut fixture.layout,
        &fixture.runtimes,
        &pane_tracked_processes,
        &fixture.terminal_size,
    )?;
    let _initial_damage = fixture.runtimes.take_screen_dirty_panes();
    // Wait for distinct intermediate output so the existing footer cannot satisfy the repaint wait early.
    fixture.write_screen_text("repaint in progress")?;
    fixture.write_screen_text(completed)?;
    test_that::assert_that!(fixture.runtimes.handle(fixture.pane_id)?.live_tail_text(12), eq(before));
    let (mut event_writer, client_drain) =
        self::connect_client_event_drain(&fixture.config, &mut render_worker).await?;
    let delete_sessions = DeleteSessions::default();
    let (pty_event_sender, _pty_event_receiver) = self::pty_event_channel();
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
    self::assert_repainted_completion_settles(
        &mut state,
        &mut event_writer,
        &mut timers,
        fixture.pane_id,
        expected_state,
    )
    .await?;
    self::abort_client_drain(client_drain).await;
    Ok(())
}

#[rstest::rstest]
#[case("Working (1s • esc to interrupt)", TrackedProcessState::Busy, true)]
#[case("Worked for 1s • 14:18", TrackedProcessState::Seen, true)]
#[case("ready for input", TrackedProcessState::Seen, false)]
fn test_codex_when_first_discovered_on_focus_seeds_visible_status(
    #[case] text: &str,
    #[case] expected: TrackedProcessState,
    #[case] has_deadline: bool,
) -> rootcause::Result<()> {
    let fixture = self::tracked_cat_runtime_fixture()?;
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
        Some(self::instant_after(now, Duration::from_secs(3))?)
    } else {
        None
    };
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(expected_deadline));
    Ok(())
}

#[rstest::rstest]
#[case("  Worked for 16m 15s • 14:18", Duration::from_secs(3))]
#[case("Working (6m 35s • ctrl+x to interrupt)", Duration::from_secs(4))]
#[case("approval required", Duration::from_secs(4))]
#[case(
    "Worked for 16m 15s • 14:18\nWorking (1s • esc to interrupt)",
    Duration::from_secs(4)
)]
fn test_record_cached_visible_activity_when_screen_status_varies_extends_only_unconfirmed_completion(
    #[case] text: &str,
    #[case] expected_delay: Duration,
) -> rootcause::Result<()> {
    let fixture = self::tracked_cat_runtime_fixture()?;
    fixture.write_screen_text("Working (0s • esc to interrupt)")?;
    let then = Instant::now();
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], then)?;

    fixture.write_screen_text(text)?;
    processes.record_cached_visible_activity(
        &fixture.runtimes,
        &[fixture.pane_id],
        self::instant_after(then, Duration::from_secs(1))?,
    )?;

    test_that::assert_that!(
        processes.next_quiet_deadline(&fixture.layout)?,
        eq(Some(self::instant_after(then, expected_delay)?))
    );
    Ok(())
}

#[rstest::rstest]
#[case(TrackedProcessState::Seen, "Worked for 22m 38s • 11:37")]
#[case(TrackedProcessState::Unseen, "Worked for 22m 38s • 11:37")]
#[case(
    TrackedProcessState::Unseen,
    "Working (6m 35s • ctrl+x to interrupt)\nWorked for 22m 38s • 11:37"
)]
fn test_tracked_process_screen_when_completion_is_latest_allows_due_attention(
    #[case] expected_state: TrackedProcessState,
    #[case] finished_text: &str,
) -> rootcause::Result<()> {
    let mut fixture = self::tracked_cat_runtime_fixture()?;
    if expected_state == TrackedProcessState::Unseen {
        fixture.layout.active_tab_mut()?.focus_pane(PaneId::new(2)?)?;
    }
    fixture.write_screen_text("Working (6m 35s • ctrl+x to interrupt)")?;
    let then = Instant::now();
    let quiet_at = self::instant_after(then, Duration::from_secs(30))?;
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], then)?;
    processes.guard_quiet_deadlines(
        &fixture.config.user_config,
        &fixture.layout,
        &fixture.runtimes,
        quiet_at,
    )?;
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, quiet_at)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(TrackedProcessState::Busy)
    );
    test_that::assert_that!(
        processes.next_quiet_deadline(&fixture.layout)?,
        eq(Some(self::instant_after(quiet_at, Duration::from_secs(3))?))
    );

    fixture.write_screen_text(finished_text)?;
    let completed_at = self::instant_after(quiet_at, Duration::from_secs(1))?;
    processes.observe_runtime_pane_cmds(
        &fixture.config.user_config,
        &fixture.runtimes,
        &[fixture.pane_id],
        completed_at,
    )?;
    processes.guard_quiet_deadlines(
        &fixture.config.user_config,
        &fixture.layout,
        &fixture.runtimes,
        completed_at,
    )?;
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, completed_at)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    let deadline = self::instant_after(quiet_at, Duration::from_secs(3))?;
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(Some(deadline)));
    processes.guard_quiet_deadlines(
        &fixture.config.user_config,
        &fixture.layout,
        &fixture.runtimes,
        deadline,
    )?;
    let expected_attention = if expected_state == TrackedProcessState::Unseen {
        TrackedProcessAttention::Unseen {
            pane_ids: vec![fixture.pane_id],
        }
    } else {
        TrackedProcessAttention::Seen
    };
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, deadline)?,
        eq(expected_attention)
    );
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(expected_state)
    );
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(None));
    Ok(())
}

#[rstest::rstest]
#[case("approval required")]
#[case("Worked for 22m 38s • 11:37\nWorking (6m 35s • ctrl+x to interrupt)")]
fn test_tracked_process_screen_when_attention_is_not_confirmed_keeps_busy_and_retries(
    #[case] text: &str,
) -> rootcause::Result<()> {
    let mut fixture = self::tracked_cat_runtime_fixture()?;
    fixture.layout.active_tab_mut()?.focus_pane(PaneId::new(2)?)?;
    fixture.write_screen_text("Working (0s • esc to interrupt)")?;
    let then = Instant::now();
    let now = self::instant_after(then, Duration::from_secs(30))?;
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], then)?;
    fixture.write_screen_text(text)?;
    processes.guard_quiet_deadlines(&fixture.config.user_config, &fixture.layout, &fixture.runtimes, now)?;
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&fixture.layout, now)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    test_that::assert_that!(
        self::tracked_process_snapshot_state(&processes.snapshot(&fixture.layout), fixture.pane_id)?,
        eq(TrackedProcessState::Busy)
    );
    test_that::assert_that!(
        processes.next_quiet_deadline(&fixture.layout)?,
        eq(Some(self::instant_after(now, Duration::from_secs(3))?))
    );
    Ok(())
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
fn test_tracked_process_screen_when_quiet_deadline_is_not_due_preserves_deadline(
    #[case] recent_input: bool,
) -> rootcause::Result<()> {
    let fixture = self::tracked_cat_runtime_fixture()?;
    let handle = fixture.runtimes.handle(fixture.pane_id)?;
    let busy = format!("{}Working (6m 35s • ctrl+x to interrupt)\n", "\n".repeat(24));
    handle.write_input(busy.as_bytes())?;
    self::wait_for_runtime_snapshot_contains(&fixture.runtimes, fixture.pane_id, "to interrupt)")?;
    let then = Instant::now();
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], then)?;
    let check_at = if recent_input {
        processes.record_user_interaction(
            &fixture.layout,
            fixture.pane_id,
            TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_secs(2))?,
        )?;
        self::instant_after(then, Duration::from_secs(3))?
    } else {
        self::instant_after(then, Duration::from_secs(1))?
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
    let mut fixture = self::tracked_cat_runtime_fixture()?;
    fixture.layout.active_tab_mut()?.focus_pane(PaneId::new(2)?)?;
    fixture.write_screen_text("Worked for 22m 38s • 11:37")?;
    let then = Instant::now();
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], then)?;
    processes.mark_quiet_deadlines(&fixture.layout, self::instant_after(then, Duration::from_secs(3))?)?;
    if acknowledged {
        processes.acknowledge_attention(fixture.pane_id);
    }

    fixture.write_screen_text("redraw")?;
    let handle = fixture.runtimes.handle(fixture.pane_id)?;
    let output = format!("{}{text}\n", "\n".repeat(24));
    handle.write_input(output.as_bytes())?;
    self::wait_for_runtime_snapshot_contains(&fixture.runtimes, fixture.pane_id, text)?;
    let now = self::instant_after(then, Duration::from_secs(30))?;
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
        Some(self::instant_after(now, Duration::from_secs(3))?)
    } else {
        None
    };
    test_that::assert_that!(processes.next_quiet_deadline(&fixture.layout)?, eq(expected_deadline));
    Ok(())
}

#[test]
fn test_tracked_process_screen_when_foreground_is_untracked_discards_stale_busy_footer() -> rootcause::Result<()> {
    let fixture = self::tracked_cat_runtime_fixture()?;
    let handle = fixture.runtimes.handle(fixture.pane_id)?;
    let busy = format!("{}Working (6m 35s • ctrl+x to interrupt)\n", "\n".repeat(24));
    handle.write_input(busy.as_bytes())?;
    self::wait_for_runtime_snapshot_contains(&fixture.runtimes, fixture.pane_id, "to interrupt)")?;
    let then = Instant::now();
    let now = self::instant_after(then, Duration::from_secs(3))?;
    let mut processes = PaneTrackedProcesses::default();
    processes.observe_runtime_pane_cmds(&fixture.config.user_config, &fixture.runtimes, &[fixture.pane_id], then)?;
    // The ordinary config does not track this fixture's cat process. Fresh process evidence must win over the
    // Codex footer left in its terminal and the previously cached Codex identity.
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
    let mut fixture = self::tracked_cat_runtime_fixture()?;
    fixture.layout.active_tab_mut()?.focus_pane(PaneId::new(2)?)?;
    let process = Arc::make_mut(&mut fixture.config.user_config)
        .tracked_processes
        .processes
        .iter_mut()
        .find(|process| process.matches("cat", None))
        .ok_or_else(|| rootcause::report!("missing configured cat process"))?;
    process.id = agent;
    process.screen_observation = Some(ScreenObservationConfig {
        busy: ObservationPatterns::try_new(vec![Regex::new(r"\ATASK\s+ACTIVE\z")?])?,
        needs_attention: ObservationPatterns::try_new(vec![Regex::new(r"\ATASK\s+DONE\b")?])?,
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
    fixture.write_screen_text("# TASK DONE turn 1 #")?;
    processes.record_cached_visible_activity(&fixture.runtimes, &[fixture.pane_id], then)?;
    let due = self::instant_after(then, Duration::from_secs(3))?;
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
    Ok(())
}
