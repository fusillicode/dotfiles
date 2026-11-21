use test_that::prelude::*;

use crate::pane::tracked_process::lifecycle::*;
use crate::pane::tracked_process::tests::instant_after;
use crate::pane::tracked_process::tests::tracked_process;

#[rstest::rstest]
#[case(PaneTrackedProcessStatus::Busy)]
#[case(PaneTrackedProcessStatus::Unseen)]
#[case(PaneTrackedProcessStatus::Seen)]
fn test_codex_when_cancelled_clears_work_and_does_not_rearm_on_repaint(
    #[case] status: PaneTrackedProcessStatus,
) -> rootcause::Result<()> {
    let now = Instant::now();
    let mut lifecycle = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process("codex")?,
        now,
    );
    lifecycle.status = status;
    lifecycle.pending_work_start = PendingTrackedWorkStart::Pending;
    lifecycle.completion_before_work = Some("old completion".to_owned());
    lifecycle.record_screen_activity(ScreenObservation::Cancelled, now);
    test_that::assert_that!(lifecycle.state(), eq(TrackedProcessState::Seen));
    test_that::assert_that!(lifecycle.quiet_deadline(TrackedProcessPaneFocus::Unfocused)?, eq(None));
    test_that::assert_that!(lifecycle.pending_work_start, eq(PendingTrackedWorkStart::None));
    test_that::assert_that!(lifecycle.completion_before_work, eq(None));
    test_that::assert_that!(
        lifecycle.record_screen_activity(ScreenObservation::Unknown, now),
        eq(TrackedProcessChanges::default())
    );
    test_that::assert_that!(
        lifecycle.guard_quiet_deadline(Some(ScreenObservation::Cancelled), now),
        eq(TrackedProcessChanges::default())
    );
    lifecycle.record_screen_activity(ScreenObservation::Busy, now);
    test_that::assert_that!(lifecycle.state(), eq(TrackedProcessState::Busy));
    Ok(())
}

#[rstest::rstest]
#[case("codex", TrackedProcessState::Seen)]
#[case("claude", TrackedProcessState::Busy)]
#[case("cursor-agent", TrackedProcessState::Busy)]
#[case("gemini", TrackedProcessState::Busy)]
#[case("opencode", TrackedProcessState::Busy)]
fn test_lifecycle_when_discovered_or_entered_uses_agent_busy_start_policy(
    #[case] executable: &str,
    #[case] expected: TrackedProcessState,
) -> rootcause::Result<()> {
    let now = Instant::now();
    let mut lifecycle = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process(executable)?,
        now,
    );
    test_that::assert_that!(lifecycle.state(), eq(expected));
    lifecycle.status = PaneTrackedProcessStatus::Seen;
    lifecycle.record_user_interaction(
        TrackedProcessUserInteraction::StartsTrackedProcessWork,
        now,
        TrackedProcessPaneFocus::Focused,
    );
    lifecycle.record_visible_activity(now);
    test_that::assert_that!(lifecycle.state(), eq(expected));
    test_that::assert_that!(
        lifecycle.quiet_deadline(TrackedProcessPaneFocus::Focused)?.is_some(),
        eq(expected == TrackedProcessState::Busy)
    );
    Ok(())
}

#[rstest::rstest]
#[case(PaneTrackedProcessStatus::Seen)]
#[case(PaneTrackedProcessStatus::Unseen)]
fn test_codex_when_working_arrives_without_enter_starts_busy_despite_local_echo(
    #[case] status: PaneTrackedProcessStatus,
) -> rootcause::Result<()> {
    let now = Instant::now();
    let mut lifecycle = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process("codex")?,
        now,
    );
    lifecycle.status = status;
    lifecycle.pending_work_start = PendingTrackedWorkStart::None;
    lifecycle.record_user_interaction(
        TrackedProcessUserInteraction::MayEcho,
        now,
        TrackedProcessPaneFocus::Focused,
    );
    test_that::assert_that!(
        lifecycle.record_screen_activity(ScreenObservation::Busy, now),
        eq(TrackedProcessChanges::state_and_deadline())
    );
    test_that::assert_that!(lifecycle.state(), eq(TrackedProcessState::Busy));
    let later = crate::pane::tracked_process::lifecycle::tests::instant_after(now, Duration::from_secs(1))?;
    lifecycle.record_screen_activity(ScreenObservation::Unknown, later);
    test_that::assert_that!(
        lifecycle.quiet_deadline(TrackedProcessPaneFocus::Unfocused)?,
        eq(Some(self::instant_after(later, Duration::from_secs(3))?))
    );
    Ok(())
}

#[test]
fn test_activity_started_process_when_old_completion_reappears_defers_quiet() -> rootcause::Result<()> {
    let now = Instant::now();
    let mut lifecycle = PaneTrackedProcessLifecycle::new(tracked_process("cursor-agent")?, now);
    let old_completion = ScreenObservation::NeedsAttention("TASK DONE turn 1");
    lifecycle.capture_completion_before_work(old_completion);
    let due = instant_after(now, Duration::from_secs(3))?;

    test_that::assert_that!(
        lifecycle.guard_quiet_deadline(Some(old_completion), due),
        eq(TrackedProcessChanges::deadline_only())
    );
    let retry = instant_after(due, Duration::from_secs(3))?;
    test_that::assert_that!(
        lifecycle.guard_quiet_deadline(Some(ScreenObservation::NeedsAttention("TASK DONE turn 2")), retry),
        eq(TrackedProcessChanges::default())
    );
    lifecycle.mark_quiet_if_due(retry, TrackedProcessPaneFocus::Unfocused);

    test_that::assert_that!(lifecycle.state(), eq(TrackedProcessState::Unseen));
    Ok(())
}

#[test]
fn test_codex_when_working_disappears_during_local_echo_extends_quiet_on_damage() -> rootcause::Result<()> {
    let now = Instant::now();
    let mut lifecycle = PaneTrackedProcessLifecycle::new(tracked_process("codex")?, now);
    lifecycle.record_screen_activity(ScreenObservation::Busy, now);
    lifecycle.record_user_interaction(
        TrackedProcessUserInteraction::MayEcho,
        now,
        TrackedProcessPaneFocus::Focused,
    );
    let output = instant_after(now, Duration::from_millis(100))?;

    lifecycle.record_screen_activity(ScreenObservation::Unknown, output);

    test_that::assert_that!(
        lifecycle.quiet_deadline(TrackedProcessPaneFocus::Unfocused)?,
        eq(Some(instant_after(output, Duration::from_secs(3))?))
    );
    Ok(())
}

#[rstest::rstest]
#[case::seen(PaneTrackedProcessStatus::Seen, TrackedProcessState::Seen)]
#[case::unseen(PaneTrackedProcessStatus::Unseen, TrackedProcessState::Unseen)]
fn test_pane_tracked_process_lifecycle_when_user_echoes_visible_activity_does_not_mark_busy(
    #[case] starting_status: PaneTrackedProcessStatus,
    #[case] expected_state: TrackedProcessState,
) -> rootcause::Result<()> {
    let then = Instant::now();
    let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process("claude")?,
        then,
    );
    pane_tracked_process.status = starting_status;
    pane_tracked_process.record_user_interaction(
        TrackedProcessUserInteraction::MayEcho,
        then,
        TrackedProcessPaneFocus::Focused,
    );

    test_that::assert_that!(
        pane_tracked_process.record_visible_activity(self::instant_after(then, Duration::from_millis(100))?,),
        eq(TrackedProcessChanges::default())
    );

    test_that::assert_that!(pane_tracked_process.state(), eq(expected_state));
    Ok(())
}

#[rstest::rstest]
#[case::seen(PaneTrackedProcessStatus::Seen)]
#[case::unseen(PaneTrackedProcessStatus::Unseen)]
fn test_pane_tracked_process_lifecycle_when_prompt_submit_without_output_marks_busy(
    #[case] starting_status: PaneTrackedProcessStatus,
) -> rootcause::Result<()> {
    let then = Instant::now();
    let prompt_submitted_at =
        crate::pane::tracked_process::lifecycle::tests::instant_after(then, Duration::from_millis(100))?;
    let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process("claude")?,
        then,
    );
    pane_tracked_process.status = starting_status;
    pane_tracked_process.record_user_interaction(
        TrackedProcessUserInteraction::MayEcho,
        then,
        TrackedProcessPaneFocus::Focused,
    );

    test_that::assert_that!(
        pane_tracked_process.record_user_interaction(
            TrackedProcessUserInteraction::StartsTrackedProcessWork,
            prompt_submitted_at,
            TrackedProcessPaneFocus::Focused,
        ),
        eq(TrackedProcessChanges::state_and_deadline())
    );

    test_that::assert_that!(pane_tracked_process.state(), eq(TrackedProcessState::Busy));
    test_that::assert_that!(
        pane_tracked_process.quiet_deadline(TrackedProcessPaneFocus::Focused)?,
        eq(Some(self::instant_after(prompt_submitted_at, Duration::from_secs(3))?))
    );
    Ok(())
}

#[test]
fn test_pane_tracked_process_lifecycle_when_busy_output_moves_only_quiet_deadline() -> rootcause::Result<()> {
    let then = Instant::now();
    let visible_activity_at =
        crate::pane::tracked_process::lifecycle::tests::instant_after(then, Duration::from_millis(501))?;
    let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process("claude")?,
        then,
    );

    test_that::assert_that!(
        pane_tracked_process.record_visible_activity(visible_activity_at),
        eq(TrackedProcessChanges::deadline_only())
    );

    test_that::assert_that!(
        pane_tracked_process.quiet_deadline(TrackedProcessPaneFocus::Unfocused)?,
        eq(Some(self::instant_after(visible_activity_at, Duration::from_secs(3))?))
    );
    Ok(())
}

#[test]
fn test_pane_tracked_process_lifecycle_when_user_echo_suppression_expires_records_busy_activity()
-> rootcause::Result<()> {
    let then = Instant::now();
    let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process("claude")?,
        then,
    );
    pane_tracked_process.record_user_interaction(
        TrackedProcessUserInteraction::MayEcho,
        then,
        TrackedProcessPaneFocus::Focused,
    );
    let visible_activity_at =
        crate::pane::tracked_process::lifecycle::tests::instant_after(then, Duration::from_millis(501))?;

    test_that::assert_that!(
        pane_tracked_process.record_visible_activity(visible_activity_at),
        eq(TrackedProcessChanges::deadline_only())
    );

    test_that::assert_that!(
        pane_tracked_process.quiet_deadline(TrackedProcessPaneFocus::Unfocused)?,
        eq(Some(self::instant_after(visible_activity_at, Duration::from_secs(3))?))
    );
    Ok(())
}

#[test]
fn test_pane_tracked_process_lifecycle_when_prompt_submit_precedes_visible_activity_marks_busy() -> rootcause::Result<()>
{
    let then = Instant::now();
    let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process("claude")?,
        then,
    );
    pane_tracked_process.status = PaneTrackedProcessStatus::Seen;
    pane_tracked_process.record_user_interaction(
        TrackedProcessUserInteraction::MayEcho,
        then,
        TrackedProcessPaneFocus::Focused,
    );
    test_that::assert_that!(
        pane_tracked_process.record_user_interaction(
            TrackedProcessUserInteraction::StartsTrackedProcessWork,
            self::instant_after(then, Duration::from_millis(100))?,
            TrackedProcessPaneFocus::Focused,
        ),
        eq(TrackedProcessChanges::state_and_deadline())
    );

    test_that::assert_that!(
        pane_tracked_process.record_visible_activity(self::instant_after(then, Duration::from_millis(150))?,),
        eq(TrackedProcessChanges::deadline_only())
    );

    test_that::assert_that!(pane_tracked_process.state(), eq(TrackedProcessState::Busy));
    Ok(())
}

#[rstest::rstest]
#[case::focused(TrackedProcessPaneFocus::Focused, TrackedProcessState::Seen)]
#[case::unfocused(TrackedProcessPaneFocus::Unfocused, TrackedProcessState::Unseen)]
fn test_pane_tracked_process_lifecycle_when_quiet_deadline_fires_marks_seen_or_unseen(
    #[case] focus_state: TrackedProcessPaneFocus,
    #[case] expected_state: TrackedProcessState,
) -> rootcause::Result<()> {
    let then = Instant::now();
    let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process("claude")?,
        then,
    );

    test_that::assert_that!(
        pane_tracked_process.mark_quiet_if_due(self::instant_after(then, Duration::from_secs(3))?, focus_state),
        eq(TrackedProcessStateChange::Changed)
    );

    test_that::assert_that!(pane_tracked_process.state(), eq(expected_state));
    Ok(())
}

#[test]
fn test_pane_tracked_process_lifecycle_when_focused_user_input_is_recent_stays_busy() -> rootcause::Result<()> {
    let then = Instant::now();
    let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process("claude")?,
        then,
    );
    test_that::assert_that!(
        pane_tracked_process.record_user_interaction(
            TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_secs(2))?,
            TrackedProcessPaneFocus::Focused,
        ),
        eq(TrackedProcessChanges::deadline_only())
    );

    test_that::assert_that!(
        pane_tracked_process.mark_quiet_if_due(
            self::instant_after(then, Duration::from_secs(3))?,
            TrackedProcessPaneFocus::Focused
        ),
        eq(TrackedProcessStateChange::Unchanged)
    );
    test_that::assert_that!(pane_tracked_process.state(), eq(TrackedProcessState::Busy));

    test_that::assert_that!(
        pane_tracked_process.mark_quiet_if_due(
            self::instant_after(then, Duration::from_secs(5))?,
            TrackedProcessPaneFocus::Focused
        ),
        eq(TrackedProcessStateChange::Changed)
    );
    test_that::assert_that!(pane_tracked_process.state(), eq(TrackedProcessState::Seen));
    Ok(())
}

#[test]
fn test_pane_tracked_process_lifecycle_when_unfocused_user_input_is_recent_still_marks_unseen() -> rootcause::Result<()>
{
    let then = Instant::now();
    let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process("claude")?,
        then,
    );
    test_that::assert_that!(
        pane_tracked_process.record_user_interaction(
            TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_secs(2))?,
            TrackedProcessPaneFocus::Unfocused,
        ),
        eq(TrackedProcessChanges::default())
    );

    test_that::assert_that!(
        pane_tracked_process.mark_quiet_if_due(
            self::instant_after(then, Duration::from_secs(3))?,
            TrackedProcessPaneFocus::Unfocused
        ),
        eq(TrackedProcessStateChange::Changed)
    );

    test_that::assert_that!(pane_tracked_process.state(), eq(TrackedProcessState::Unseen));
    Ok(())
}

#[test]
fn test_pane_tracked_process_lifecycle_when_attention_is_acknowledged_marks_seen() -> rootcause::Result<()> {
    let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(
        crate::pane::tracked_process::lifecycle::tests::tracked_process("claude")?,
        Instant::now(),
    );
    pane_tracked_process.status = PaneTrackedProcessStatus::Unseen;

    test_that::assert_that!(
        pane_tracked_process.acknowledge_attention(),
        eq(TrackedProcessStateChange::Changed)
    );

    test_that::assert_that!(pane_tracked_process.state(), eq(TrackedProcessState::Seen));
    Ok(())
}
