use std::time::Duration;

use muxr_config::TrackedProcessId;
use muxr_core::SessionName;
use muxr_core::TerminalSize;
use test_that::prelude::*;

use super::*;
use crate::pane::cmd::PaneCmd;
use crate::pane::cmd::PaneCmdUnknownReason;
use crate::pane::split::PaneSplitAxis;
use crate::state::SessionMetadata;
use crate::terminal::TerminalState;

pub(super) fn tracked_process(executable: &str) -> rootcause::Result<TrackedProcess> {
    MuxrConfig::new()?
        .tracked_process_for_cmd(executable, None)
        .cloned()
        .ok_or_else(|| rootcause::report!("expected configured tracked process"))
}

pub(super) fn instant_after(instant: Instant, duration: Duration) -> rootcause::Result<Instant> {
    instant
        .checked_add(duration)
        .ok_or_else(|| rootcause::report!("test instant overflowed"))
}

#[test]
fn test_lifecycle_when_screen_observation_is_disabled_uses_activity_tracking() -> rootcause::Result<()> {
    let mut config = MuxrConfig::new()?;
    let codex = config
        .tracked_processes
        .processes
        .iter_mut()
        .find(|process| process.id == TrackedProcessId::Codex)
        .ok_or_else(|| rootcause::report!("missing Codex config"))?;
    codex.screen_observation = None;
    let mut processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    processes.observe_pane_cmd(&config, pane_id, &self::fg_tracked_process("codex"), Instant::now());
    test_that::assert_that!(
        pane_tracked_process_status(&processes, pane_id),
        eq(TrackedProcessState::Busy)
    );
    Ok(())
}

fn pane_tracked_process_status(pane_tracked_processes: &PaneTrackedProcesses, pane_id: PaneId) -> TrackedProcessState {
    pane_tracked_processes
        .by_pane
        .get(&pane_id)
        .map_or(TrackedProcessState::None, PaneTrackedProcessLifecycle::state)
}

fn mark_pane_quiet(
    processes: &mut PaneTrackedProcesses,
    pane_id: PaneId,
    focus: TrackedProcessPaneFocus,
) -> rootcause::Result<()> {
    let lifecycle = processes
        .by_pane
        .get_mut(&pane_id)
        .ok_or_else(|| rootcause::report!("missing test lifecycle"))?;
    let deadline = lifecycle
        .quiet_deadline(focus)?
        .ok_or_else(|| rootcause::report!("test lifecycle has no quiet deadline"))?;
    lifecycle.mark_quiet_if_due(deadline, focus);
    Ok(())
}

#[test]
fn test_observe_pane_cmd_when_tracked_process_is_fg_marks_busy() -> rootcause::Result<()> {
    let layout = self::layout()?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;

    test_that::assert_that!(
        pane_tracked_processes
            .observe_pane_cmd(
                &MuxrConfig::new()?,
                pane_id,
                &self::fg_tracked_process("claude"),
                Instant::now(),
            )
            .state_change()
            == TrackedProcessStateChange::Changed,
        eq(true)
    );

    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::Busy)
    );
    let snapshot = pane_tracked_processes.snapshot(&layout);
    let pane = self::tracked_process_snapshot_pane(&snapshot, pane_id)?;
    test_that::assert_that!(pane.label(), eq("cl"));
    test_that::assert_that!(pane.state(), eq(TrackedProcessState::Busy));
    Ok(())
}

#[test]
fn test_observe_pane_cmd_when_tracked_process_is_fg_group_member_marks_busy() -> rootcause::Result<()> {
    let layout = self::layout()?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;

    test_that::assert_that!(
        pane_tracked_processes
            .observe_pane_cmd(
                &MuxrConfig::new()?,
                pane_id,
                &PaneCmdObservation::FgCmd(FgCmd::from_test_group(
                    Some(self::cmd(17869, "agg")),
                    Ok(vec![self::cmd(17989, "claude")]),
                )),
                Instant::now(),
            )
            .state_change()
            == TrackedProcessStateChange::Changed,
        eq(true)
    );

    let snapshot = pane_tracked_processes.snapshot(&layout);
    let pane = self::tracked_process_snapshot_pane(&snapshot, pane_id)?;
    test_that::assert_that!(pane.label(), eq("cl"));
    test_that::assert_that!(pane.state(), eq(TrackedProcessState::Busy));
    Ok(())
}

#[rstest::rstest]
#[case::shell(self::shell())]
#[case::non_tracked(self::fg_cmd("nvim"))]
fn test_observe_pane_cmd_when_trusted_untracked_cmd_clears_state(
    #[case] observation: PaneCmdObservation,
) -> rootcause::Result<()> {
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);
    self::mark_pane_quiet(&mut pane_tracked_processes, pane_id, TrackedProcessPaneFocus::Unfocused)?;

    test_that::assert_that!(
        pane_tracked_processes
            .observe_pane_cmd(
                &MuxrConfig::new()?,
                pane_id,
                &observation,
                self::instant_after(then, Duration::from_secs(1))?,
            )
            .state_change()
            == TrackedProcessStateChange::Changed,
        eq(true)
    );

    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::None)
    );
    Ok(())
}

#[rstest::rstest]
#[case::shell(self::shell())]
#[case::non_tracked(self::fg_cmd("nvim"))]
fn test_observe_visible_activity_when_trusted_untracked_cmd_clears_state(
    #[case] observation: PaneCmdObservation,
) -> rootcause::Result<()> {
    let layout = self::layout()?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);

    test_that::assert_that!(
        pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::new()?,
            pane_id,
            &observation,
            self::instant_after(then, Duration::from_secs(1))?,
        ),
        eq(TrackedProcessChanges::state_and_deadline())
    );

    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::None)
    );
    let snapshot = pane_tracked_processes.snapshot(&layout);
    test_that::assert_that!(self::tracked_process_snapshot_pane(&snapshot, pane_id), err(anything()));
    test_that::assert_that!(pane_tracked_processes.next_quiet_deadline(&layout)?, eq(None));
    Ok(())
}

#[test]
fn test_observe_pane_cmd_when_unknown_preserves_state() -> rootcause::Result<()> {
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);
    self::mark_pane_quiet(&mut pane_tracked_processes, pane_id, TrackedProcessPaneFocus::Unfocused)?;

    test_that::assert_that!(
        pane_tracked_processes
            .observe_pane_cmd(
                &MuxrConfig::new()?,
                pane_id,
                &self::unknown(),
                self::instant_after(then, Duration::from_secs(1))?,
            )
            .presence()
            == TrackedProcessChangePresence::Empty,
        eq(true)
    );

    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::Unseen)
    );
    Ok(())
}

#[test]
fn test_observe_visible_activity_when_unseen_tracked_process_repaints_without_prompt_keeps_attention()
-> rootcause::Result<()> {
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    pane_tracked_processes.observe_pane_cmd(
        &MuxrConfig::new()?,
        pane_id,
        &self::fg_tracked_process("claude"),
        Instant::now(),
    );
    self::mark_pane_quiet(&mut pane_tracked_processes, pane_id, TrackedProcessPaneFocus::Unfocused)?;

    test_that::assert_that!(
        pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::new()?,
            pane_id,
            &self::fg_tracked_process("claude"),
            Instant::now(),
        ),
        eq(TrackedProcessChanges::default())
    );

    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::Unseen)
    );
    Ok(())
}

#[test]
fn test_observe_visible_activity_when_cursor_repaints_after_seen_does_not_mark_busy() -> rootcause::Result<()> {
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    pane_tracked_processes.observe_pane_cmd(
        &MuxrConfig::new()?,
        pane_id,
        &self::fg_tracked_process("cursor-agent"),
        Instant::now(),
    );
    self::mark_pane_quiet(&mut pane_tracked_processes, pane_id, TrackedProcessPaneFocus::Focused)?;

    test_that::assert_that!(
        pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::new()?,
            pane_id,
            &self::fg_tracked_process("cursor-agent"),
            Instant::now(),
        ),
        eq(TrackedProcessChanges::default())
    );

    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::Seen)
    );
    Ok(())
}

#[test]
fn test_observe_visible_activity_when_user_echoes_output_does_not_mark_busy() -> rootcause::Result<()> {
    let layout = self::layout()?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);
    self::mark_pane_quiet(&mut pane_tracked_processes, pane_id, TrackedProcessPaneFocus::Focused)?;
    pane_tracked_processes.record_user_interaction(&layout, pane_id, TrackedProcessUserInteraction::MayEcho, then)?;

    test_that::assert_that!(
        pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::new()?,
            pane_id,
            &self::fg_tracked_process("claude"),
            self::instant_after(then, Duration::from_millis(100))?,
        ),
        eq(TrackedProcessChanges::default())
    );

    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::Seen)
    );
    Ok(())
}

#[test]
fn test_observe_pane_cmd_when_delayed_after_user_echo_does_not_extend_quiet_deadline() -> rootcause::Result<()> {
    let layout = self::layout()?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);
    pane_tracked_processes.record_user_interaction(
        &layout,
        pane_id,
        TrackedProcessUserInteraction::MayEcho,
        self::instant_after(then, Duration::from_millis(1))?,
    )?;
    let quiet_deadline = pane_tracked_processes.next_quiet_deadline(&layout)?;

    test_that::assert_that!(
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::new()?,
            pane_id,
            &self::fg_tracked_process("claude"),
            self::instant_after(then, Duration::from_millis(502))?,
        ),
        eq(TrackedProcessChanges::default())
    );
    test_that::assert_that!(pane_tracked_processes.next_quiet_deadline(&layout)?, eq(quiet_deadline));
    Ok(())
}

#[test]
fn test_observe_visible_activity_when_prompt_submit_precedes_output_marks_busy() -> rootcause::Result<()> {
    let mut layout = self::layout()?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    layout.active_tab_mut()?.focus_pane(pane_id)?;
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);
    self::mark_pane_quiet(&mut pane_tracked_processes, pane_id, TrackedProcessPaneFocus::Focused)?;
    pane_tracked_processes.record_user_interaction(&layout, pane_id, TrackedProcessUserInteraction::MayEcho, then)?;
    pane_tracked_processes.record_user_interaction(
        &layout,
        pane_id,
        TrackedProcessUserInteraction::StartsTrackedProcessWork,
        self::instant_after(then, Duration::from_millis(100))?,
    )?;

    test_that::assert_that!(
        pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::new()?,
            pane_id,
            &self::fg_tracked_process("claude"),
            self::instant_after(then, Duration::from_millis(150))?,
        ),
        eq(TrackedProcessChanges::deadline_only())
    );

    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::Busy)
    );
    Ok(())
}

#[test]
fn test_observe_visible_activity_when_prompt_submit_precedes_output_keeps_busy() -> rootcause::Result<()> {
    let mut layout = self::layout()?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    layout.active_tab_mut()?.focus_pane(pane_id)?;
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);
    self::mark_pane_quiet(&mut pane_tracked_processes, pane_id, TrackedProcessPaneFocus::Focused)?;
    pane_tracked_processes.record_user_interaction(
        &layout,
        pane_id,
        TrackedProcessUserInteraction::StartsTrackedProcessWork,
        self::instant_after(then, Duration::from_millis(100))?,
    )?;

    test_that::assert_that!(
        pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::new()?,
            pane_id,
            &self::fg_tracked_process("claude"),
            self::instant_after(then, Duration::from_millis(150))?,
        ),
        eq(TrackedProcessChanges::deadline_only())
    );
    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::Busy)
    );
    Ok(())
}

#[test]
fn test_observe_pane_cmd_when_tracked_process_identity_changes_resets_state() -> rootcause::Result<()> {
    let mut layout = self::layout()?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    layout.active_tab_mut()?.focus_pane(pane_id)?;
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);
    pane_tracked_processes.record_user_interaction(
        &layout,
        pane_id,
        TrackedProcessUserInteraction::MayEcho,
        self::instant_after(then, Duration::from_millis(100))?,
    )?;

    test_that::assert_that!(
        pane_tracked_processes
            .observe_pane_cmd(
                &MuxrConfig::new()?,
                pane_id,
                &self::fg_tracked_process("cursor-agent"),
                self::instant_after(then, Duration::from_millis(150))?,
            )
            .state_change()
            == TrackedProcessStateChange::Changed,
        eq(true)
    );
    self::mark_pane_quiet(&mut pane_tracked_processes, pane_id, TrackedProcessPaneFocus::Focused)?;
    pane_tracked_processes.record_user_interaction(
        &layout,
        pane_id,
        TrackedProcessUserInteraction::StartsTrackedProcessWork,
        self::instant_after(then, Duration::from_millis(175))?,
    )?;
    test_that::assert_that!(
        pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::new()?,
            pane_id,
            &self::fg_tracked_process("cursor-agent"),
            self::instant_after(then, Duration::from_millis(200))?,
        ),
        eq(TrackedProcessChanges::deadline_only())
    );

    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::Busy)
    );
    let snapshot = pane_tracked_processes.snapshot(&layout);
    let pane = self::tracked_process_snapshot_pane(&snapshot, pane_id)?;
    test_that::assert_that!(pane.label(), eq("cu"));
    test_that::assert_that!(pane.state(), eq(TrackedProcessState::Busy));
    Ok(())
}

#[test]
fn test_mark_quiet_deadlines_when_unfocused_busy_tracked_process_is_quiet_marks_unseen() -> rootcause::Result<()> {
    let layout = self::layout()?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = PaneId::new(1)?;
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);

    test_that::assert_that!(
        pane_tracked_processes.next_quiet_deadline(&layout)?,
        eq(Some(self::instant_after(then, Duration::from_secs(3))?))
    );
    let outcome =
        pane_tracked_processes.mark_quiet_deadlines(&layout, self::instant_after(then, Duration::from_secs(3))?)?;
    test_that::assert_that!(
        outcome,
        eq(TrackedProcessAttention::Unseen {
            pane_ids: vec![pane_id]
        })
    );

    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::Unseen)
    );
    test_that::assert_that!(pane_tracked_processes.attention_pane_ids(&layout), eq(vec![pane_id]));
    test_that::assert_that!(pane_tracked_processes.next_quiet_deadline(&layout)?, eq(None));
    Ok(())
}

#[test]
fn test_next_quiet_deadline_when_focused_user_input_is_recent_uses_user_input() -> rootcause::Result<()> {
    let mut layout = self::layout()?;
    let pane_id = PaneId::new(1)?;
    layout.active_tab_mut()?.focus_pane(pane_id)?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);
    pane_tracked_processes.record_user_interaction(
        &layout,
        pane_id,
        TrackedProcessUserInteraction::MayEcho,
        self::instant_after(then, Duration::from_secs(2))?,
    )?;

    test_that::assert_that!(
        pane_tracked_processes.next_quiet_deadline(&layout)?,
        eq(Some(self::instant_after(then, Duration::from_secs(5))?))
    );
    Ok(())
}

#[test]
fn test_next_quiet_deadline_when_unfocused_user_input_precedes_focus_uses_visible_activity() -> rootcause::Result<()> {
    let mut layout = self::layout()?;
    let pane_id = PaneId::new(1)?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);
    pane_tracked_processes.record_user_interaction(
        &layout,
        pane_id,
        TrackedProcessUserInteraction::MayEcho,
        self::instant_after(then, Duration::from_secs(2))?,
    )?;

    layout.active_tab_mut()?.focus_pane(pane_id)?;

    test_that::assert_that!(
        pane_tracked_processes.next_quiet_deadline(&layout)?,
        eq(Some(self::instant_after(then, Duration::from_secs(3))?))
    );
    Ok(())
}

#[test]
fn test_mark_quiet_deadlines_when_focused_busy_tracked_process_is_quiet_marks_seen() -> rootcause::Result<()> {
    let mut layout = self::layout()?;
    let pane_id = PaneId::new(1)?;
    layout.active_tab_mut()?.focus_pane(pane_id)?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(&MuxrConfig::new()?, pane_id, &self::fg_tracked_process("claude"), then);

    let outcome =
        pane_tracked_processes.mark_quiet_deadlines(&layout, self::instant_after(then, Duration::from_secs(3))?)?;

    test_that::assert_that!(outcome, eq(TrackedProcessAttention::Seen));
    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::Seen)
    );
    Ok(())
}

#[test]
fn test_remove_pane_when_reused_id_does_not_project_stale_tracked_state() -> rootcause::Result<()> {
    let mut layout = self::layout()?;
    let reused_pane_id = PaneId::new(2)?;
    layout.active_tab_mut()?.focus_pane(reused_pane_id)?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    pane_tracked_processes.observe_pane_cmd(
        &MuxrConfig::new()?,
        reused_pane_id,
        &self::fg_tracked_process("claude"),
        Instant::now(),
    );

    test_that::assert_that!(
        pane_tracked_processes.remove_pane(reused_pane_id).state_change() == TrackedProcessStateChange::Changed,
        eq(true)
    );
    layout.remove_exited_pane(
        reused_pane_id,
        0,
        crate::pty::PtyExitStatus {
            code: 0,
            signal: None,
            result: crate::pty::PtyExitResult::Succeeded,
        },
    )?;
    let new_pane_id = layout.split_active_pane(
        MuxrConfig::new()?.layout,
        self::metadata("sh", 3),
        PaneSplitAxis::Vertical,
    )?;

    test_that::assert_that!(new_pane_id, eq(reused_pane_id));
    let snapshot = pane_tracked_processes.snapshot(&layout);
    test_that::assert_that!(
        self::tracked_process_snapshot_pane(&snapshot, reused_pane_id),
        err(anything())
    );
    test_that::assert_that!(pane_tracked_processes.next_quiet_deadline(&layout)?, eq(None));
    Ok(())
}

#[test]
fn test_next_quiet_deadline_when_state_references_removed_pane_ignores_stale_pane() -> rootcause::Result<()> {
    let mut layout = self::layout()?;
    let stale_pane_id = PaneId::new(2)?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    pane_tracked_processes.observe_pane_cmd(
        &MuxrConfig::new()?,
        stale_pane_id,
        &self::fg_tracked_process("claude"),
        Instant::now(),
    );

    layout.remove_exited_pane(stale_pane_id, 0, self::successful_exit_status())?;

    test_that::assert_that!(pane_tracked_processes.next_quiet_deadline(&layout)?, eq(None));
    Ok(())
}

#[test]
fn test_next_quiet_deadline_when_stale_pane_deadline_is_earlier_uses_live_pane() -> rootcause::Result<()> {
    let mut layout = self::layout()?;
    let live_pane_id = PaneId::new(1)?;
    let stale_pane_id = PaneId::new(2)?;
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let then = Instant::now();
    pane_tracked_processes.observe_pane_cmd(
        &MuxrConfig::new()?,
        stale_pane_id,
        &self::fg_tracked_process("claude"),
        then,
    );
    pane_tracked_processes.observe_pane_cmd(
        &MuxrConfig::new()?,
        live_pane_id,
        &self::fg_tracked_process("claude"),
        self::instant_after(then, Duration::from_secs(1))?,
    );
    layout.remove_exited_pane(stale_pane_id, 0, self::successful_exit_status())?;

    test_that::assert_that!(
        pane_tracked_processes.next_quiet_deadline(&layout)?,
        eq(Some(self::instant_after(then, Duration::from_secs(4))?))
    );
    Ok(())
}

#[test]
fn test_acknowledge_attention_when_tracked_process_is_unseen_marks_seen() -> rootcause::Result<()> {
    let mut pane_tracked_processes = PaneTrackedProcesses::default();
    let pane_id = self::pane_id()?;
    pane_tracked_processes.observe_pane_cmd(
        &MuxrConfig::new()?,
        pane_id,
        &self::fg_tracked_process("claude"),
        Instant::now(),
    );
    self::mark_pane_quiet(&mut pane_tracked_processes, pane_id, TrackedProcessPaneFocus::Unfocused)?;

    test_that::assert_that!(
        pane_tracked_processes.acknowledge_attention(pane_id).state_change() == TrackedProcessStateChange::Changed,
        eq(true)
    );

    test_that::assert_that!(
        pane_tracked_process_status(&pane_tracked_processes, pane_id),
        eq(TrackedProcessState::Seen)
    );
    Ok(())
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
fn test_guard_quiet_deadlines_when_identity_is_unknown_keeps_busy_until_confirmed(
    #[case] focused: bool,
) -> rootcause::Result<()> {
    let mut layout = self::layout()?;
    let pane_id = self::pane_id()?;
    if focused {
        layout.active_tab_mut()?.focus_pane(pane_id)?;
    }
    let then = Instant::now();
    let due = self::instant_after(then, Duration::from_secs(3))?;
    let mut processes = PaneTrackedProcesses::default();
    let process = self::tracked_process("codex")?;
    processes.apply_cmd_observation(pane_id, TrackedProcessCmdObservation::Tracked(&process), then);
    processes
        .by_pane
        .get_mut(&pane_id)
        .ok_or_else(|| rootcause::report!("missing lifecycle"))?
        .record_screen_activity(ScreenObservation::Busy, then);
    test_that::assert_that!(
        processes.guard_observed_quiet_deadlines(
            &layout,
            due,
            |_| Ok(TrackedProcessCmdObservation::Unknown),
            |_| Err(rootcause::report!("unconfirmed identity must not read stale screen")),
        )?,
        eq(TrackedProcessChanges::deadline_only())
    );
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&layout, due)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    let retry = self::instant_after(due, Duration::from_secs(3))?;
    test_that::assert_that!(processes.next_quiet_deadline(&layout)?, eq(Some(retry)));
    processes.guard_observed_quiet_deadlines(
        &layout,
        retry,
        |_| Ok(TrackedProcessCmdObservation::Tracked(&process)),
        |_| self::screen_tail("Working (1s • esc to interrupt)"),
    )?;
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&layout, retry)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    let completed = self::instant_after(retry, Duration::from_secs(3))?;
    processes.guard_observed_quiet_deadlines(
        &layout,
        completed,
        |_| Ok(TrackedProcessCmdObservation::Tracked(&process)),
        |_| self::screen_tail("Worked for 2s • 14:19"),
    )?;
    let expected = if focused {
        TrackedProcessAttention::Seen
    } else {
        TrackedProcessAttention::Unseen {
            pane_ids: vec![pane_id],
        }
    };
    test_that::assert_that!(processes.mark_quiet_deadlines(&layout, completed)?, eq(expected));
    test_that::assert_that!(processes.next_quiet_deadline(&layout)?, eq(None));
    Ok(())
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
fn test_guard_quiet_deadlines_when_cancellation_splits_words_clears_work(
    #[case] focused: bool,
) -> rootcause::Result<()> {
    let mut layout = self::layout()?;
    let pane_id = self::pane_id()?;
    if focused {
        layout.active_tab_mut()?.focus_pane(pane_id)?;
    }
    let now = Instant::now();
    let due = self::instant_after(now, Duration::from_secs(3))?;
    let process = self::tracked_process("codex")?;
    let mut processes = PaneTrackedProcesses::default();
    processes.apply_cmd_observation(pane_id, TrackedProcessCmdObservation::Tracked(&process), now);
    processes
        .by_pane
        .get_mut(&pane_id)
        .ok_or_else(|| rootcause::report!("missing lifecycle"))?
        .record_screen_activity(ScreenObservation::Busy, now);
    let mut terminal = TerminalState::with_scrollback(&TerminalSize::new(10, 12)?, MuxrConfig::new()?.scrollback);
    let _output = terminal.process(
        "■\r\nConversati\r\non\r\ninterrupte\r\nd - use\r\n/feedback\r\nif\r\nsomething\r\nwent wrong".as_bytes(),
    );
    test_that::assert_that!(
        processes.guard_observed_quiet_deadlines(
            &layout,
            due,
            |_| Ok(TrackedProcessCmdObservation::Tracked(&process)),
            |_| Ok(terminal.live_tail_text(screen::SCREEN_TAIL_ROWS)),
        )?,
        eq(TrackedProcessChanges::state_and_deadline())
    );
    let snapshot = processes.snapshot(&layout);
    test_that::assert_that!(
        self::tracked_process_snapshot_pane(&snapshot, pane_id)?.state(),
        eq(TrackedProcessState::Seen)
    );
    test_that::assert_that!(processes.next_quiet_deadline(&layout)?, eq(None));
    test_that::assert_that!(
        processes.mark_quiet_deadlines(&layout, due)?,
        eq(TrackedProcessAttention::Unchanged)
    );
    test_that::assert_that!(processes.attention_pane_ids(&layout), eq(Vec::new()));
    Ok(())
}

fn screen_tail(text: &str) -> rootcause::Result<TerminalTextTail> {
    let mut terminal = TerminalState::with_scrollback(&TerminalSize::new(80, 1)?, MuxrConfig::new()?.scrollback);
    let _output = terminal.process(text.as_bytes());
    Ok(terminal.live_tail_text(screen::SCREEN_TAIL_ROWS))
}

fn pane_id() -> rootcause::Result<PaneId> {
    PaneId::new(1)
}

fn tracked_process_snapshot_pane(
    snapshot: &PaneTrackedProcessSnapshot,
    pane_id: PaneId,
) -> rootcause::Result<&PaneTrackedProcessSnapshotEntry> {
    snapshot
        .panes()
        .find(|(snapshot_pane_id, _pane)| *snapshot_pane_id == pane_id)
        .map(|(_pane_id, pane)| pane)
        .ok_or_else(|| rootcause::report!("expected tracked process pane snapshot"))
}

fn layout() -> rootcause::Result<SessionLayout> {
    let session: SessionName = "work".parse()?;
    let mut layout = SessionLayout::initial(&session, self::metadata("sh", 1))?;
    layout.split_active_pane(
        MuxrConfig::new()?.layout,
        self::metadata("sh", 2),
        PaneSplitAxis::Vertical,
    )?;
    Ok(layout)
}

fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
    SessionMetadata {
        cmd_label: cmd_label.to_owned(),
        cwd: "/tmp".to_owned(),
        started_at,
    }
}

fn fg_tracked_process(executable: &str) -> PaneCmdObservation {
    self::fg_cmd(executable)
}

fn fg_cmd(executable: &str) -> PaneCmdObservation {
    PaneCmdObservation::FgCmd(FgCmd::from_test_cmd(self::cmd(42, executable)))
}

fn cmd(pid: u32, executable: &str) -> PaneCmd {
    PaneCmd {
        executable: executable.to_owned(),
        path: None,
        pid,
    }
}

fn shell() -> PaneCmdObservation {
    PaneCmdObservation::Shell
}

fn unknown() -> PaneCmdObservation {
    PaneCmdObservation::Unknown {
        reason: PaneCmdUnknownReason::MissingFgProcessGroup,
    }
}

fn successful_exit_status() -> crate::pty::PtyExitStatus {
    crate::pty::PtyExitStatus {
        code: 0,
        signal: None,
        result: crate::pty::PtyExitResult::Succeeded,
    }
}
