use std::time::Duration;
use std::time::Instant;

use muxr_config::ScreenObservationConfig;
use muxr_config::TrackedProcess;
use muxr_core::TrackedProcessState;

use super::TrackedProcessChanges;
use super::TrackedProcessStateChange;
use super::TrackedProcessUserInteraction;
use super::screen;
use super::screen::BusyStart;
use super::screen::ScreenObservation;

#[cfg(test)]
mod tests;

const USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub(super) struct PaneTrackedProcessLifecycle {
    completion_before_work: Option<String>,
    last_focused_user_interaction: Option<Instant>,
    last_tracked_activity: Instant,
    pending_work_start: PendingTrackedWorkStart,
    recent_user_interaction: Option<Instant>,
    status: PaneTrackedProcessStatus,
    tracked_process: TrackedProcess,
}

impl PaneTrackedProcessLifecycle {
    pub(super) const fn new(tracked_process: TrackedProcess, now: Instant) -> Self {
        let (status, pending_work_start) = match screen::busy_start(&tracked_process) {
            BusyStart::Activity => (PaneTrackedProcessStatus::Busy, PendingTrackedWorkStart::None),
            BusyStart::Screen => (PaneTrackedProcessStatus::Seen, PendingTrackedWorkStart::Pending),
        };
        Self {
            completion_before_work: None,
            last_focused_user_interaction: None,
            last_tracked_activity: now,
            pending_work_start,
            recent_user_interaction: None,
            status,
            tracked_process,
        }
    }

    pub(super) fn observe_tracked_process(
        &mut self,
        tracked_process: &TrackedProcess,
        now: Instant,
    ) -> TrackedProcessStateChange {
        if self.tracked_process.id == tracked_process.id {
            return TrackedProcessStateChange::Unchanged;
        }

        // A different tracked foreground process starts a new lifecycle; old activity, attention, and local-echo
        // suppression belonged to the previous process and must not carry over.
        *self = Self::new(tracked_process.clone(), now);
        TrackedProcessStateChange::Changed
    }

    pub(super) fn record_user_interaction(
        &mut self,
        interaction: TrackedProcessUserInteraction,
        now: Instant,
        focus_state: TrackedProcessPaneFocus,
    ) -> TrackedProcessChanges {
        // Focused local echo does not change sidebar state, but it can still extend the quiet deadline.
        let focused_deadline_extended = focus_state == TrackedProcessPaneFocus::Focused
            && self.has_quiet_deadline()
            && now > self.quiet_activity_at(focus_state);
        if focus_state == TrackedProcessPaneFocus::Focused {
            self.last_focused_user_interaction = Some(now);
        }
        match interaction {
            TrackedProcessUserInteraction::MayEcho => {
                self.recent_user_interaction = Some(now);
                if focused_deadline_extended {
                    TrackedProcessChanges::deadline_only()
                } else {
                    TrackedProcessChanges::default()
                }
            }
            TrackedProcessUserInteraction::StartsTrackedProcessWork => {
                self.recent_user_interaction = None;
                if screen::busy_start(&self.tracked_process) == BusyStart::Screen {
                    // Enter may be an empty prompt or another editor action. Only a Working row starts green.
                    if !self.has_quiet_deadline() {
                        self.pending_work_start = PendingTrackedWorkStart::Pending;
                    }
                    return if focused_deadline_extended {
                        TrackedProcessChanges::deadline_only()
                    } else {
                        TrackedProcessChanges::default()
                    };
                }
                // Prompt submit starts tracked work even before output and anchors its quiet deadline.
                self.pending_work_start = PendingTrackedWorkStart::Pending;
                self.last_tracked_activity = now;
                TrackedProcessChanges::for_activity(self.mark_visible_activity())
            }
        }
    }

    pub(super) fn record_screen_activity(
        &mut self,
        observation: ScreenObservation<'_>,
        now: Instant,
    ) -> TrackedProcessChanges {
        match observation {
            ScreenObservation::Busy => {
                // Positive work evidence takes precedence over local-echo suppression and needs no Enter event.
                self.completion_before_work = None;
                self.pending_work_start = PendingTrackedWorkStart::None;
                self.recent_user_interaction = None;
                self.last_tracked_activity = now;
                TrackedProcessChanges::for_activity(self.mark_visible_activity())
            }
            ScreenObservation::NeedsAttention(_) if self.screen_allows_quiet(observation) => {
                let pending = self.pending_work_start;
                self.pending_work_start = PendingTrackedWorkStart::None;
                if !self.has_quiet_deadline() && pending == PendingTrackedWorkStart::Pending {
                    self.status = PaneTrackedProcessStatus::Settling;
                    self.last_tracked_activity = now;
                    return TrackedProcessChanges::state_and_deadline();
                }
                // Completed TUIs can repaint unchanged cells. Do not postpone their quiet deadline.
                TrackedProcessChanges::default()
            }
            ScreenObservation::NeedsAttention(_) | ScreenObservation::Unknown => self.record_visible_activity(now),
        }
    }

    pub(super) fn record_visible_activity(&mut self, now: Instant) -> TrackedProcessChanges {
        if screen::busy_start(&self.tracked_process) == BusyStart::Screen
            && self.status != PaneTrackedProcessStatus::Busy
        {
            return TrackedProcessChanges::default();
        }
        self.discard_stale_user_interaction(now);
        if self.recent_user_interaction.is_some() {
            // User typing and mouse gestures can redraw through the PTY. Those bytes still render, but they are not
            // tracked-process work and must not flip attention back to Busy. Keep suppression for the short window;
            // prompt submit clears it explicitly with `StartsTrackedProcessWork`.
            return TrackedProcessChanges::default();
        }
        if self.status != PaneTrackedProcessStatus::Busy && self.pending_work_start == PendingTrackedWorkStart::None {
            // Some terminal apps can repaint idle UI while unfocused. After startup/work has been acknowledged, only
            // a prompt submit is allowed to re-arm tracked-process attention from visible output.
            return TrackedProcessChanges::default();
        }

        self.pending_work_start = PendingTrackedWorkStart::None;
        self.last_tracked_activity = now;
        TrackedProcessChanges::for_activity(self.mark_visible_activity())
    }

    pub(super) fn mark_quiet_if_due(
        &mut self,
        now: Instant,
        focus_state: TrackedProcessPaneFocus,
    ) -> TrackedProcessStateChange {
        self.mark_quiet(
            now.saturating_duration_since(self.quiet_activity_at(focus_state)),
            focus_state,
        )
    }

    pub(super) const fn attention_need(&self) -> TrackedProcessAttentionNeed {
        match self.status {
            PaneTrackedProcessStatus::Unseen => TrackedProcessAttentionNeed::NeedsAttention,
            PaneTrackedProcessStatus::Busy | PaneTrackedProcessStatus::Seen | PaneTrackedProcessStatus::Settling => {
                TrackedProcessAttentionNeed::None
            }
        }
    }

    pub(super) fn state(&self) -> TrackedProcessState {
        self.status.into()
    }

    pub(super) const fn acknowledge_attention(&mut self) -> TrackedProcessStateChange {
        if !matches!(self.attention_need(), TrackedProcessAttentionNeed::NeedsAttention) {
            return TrackedProcessStateChange::Unchanged;
        }
        self.status = PaneTrackedProcessStatus::Seen;
        TrackedProcessStateChange::Changed
    }

    pub(super) fn discard_stale_user_interaction(&mut self, now: Instant) {
        let Some(last_activity) = self.recent_user_interaction else {
            return;
        };
        if now.saturating_duration_since(last_activity) > USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION {
            self.recent_user_interaction = None;
        }
    }

    pub(super) fn quiet_deadline(&self, focus_state: TrackedProcessPaneFocus) -> rootcause::Result<Option<Instant>> {
        if !self.has_quiet_deadline() {
            return Ok(None);
        }
        self.quiet_activity_at(focus_state)
            .checked_add(self.tracked_process.quiet_threshold)
            .map(Some)
            .ok_or_else(|| rootcause::report!("muxr tracked-process quiet deadline overflowed"))
    }

    pub(super) const fn label(&self) -> &'static str {
        self.tracked_process.label
    }

    pub(super) const fn screen_observation(&self) -> Option<&ScreenObservationConfig> {
        self.tracked_process.screen_observation.as_ref()
    }

    pub(super) const fn completion_capture_patterns(&self) -> Option<&ScreenObservationConfig> {
        if matches!(screen::busy_start(&self.tracked_process), BusyStart::Screen) && self.has_quiet_deadline() {
            // An editor action during work or completion must not invalidate the completion we are waiting for.
            return None;
        }
        self.screen_observation()
    }

    pub(super) fn capture_completion_before_work(&mut self, observation: ScreenObservation<'_>) {
        match observation {
            ScreenObservation::NeedsAttention(completion) => {
                self.completion_before_work = Some(completion.to_owned());
            }
            ScreenObservation::Busy => self.completion_before_work = None,
            ScreenObservation::Unknown => {}
        }
    }

    pub(super) fn guard_quiet_deadline(
        &mut self,
        observation: ScreenObservation<'_>,
        now: Instant,
    ) -> TrackedProcessChanges {
        let mut changes = TrackedProcessChanges::default();
        if matches!(observation, ScreenObservation::Busy) {
            changes.merge(self.record_screen_activity(observation, now));
        }
        if !self.screen_allows_quiet(observation) {
            self.last_tracked_activity = now;
            changes.merge(TrackedProcessChanges::deadline_only());
        }
        changes
    }

    fn screen_allows_quiet(&mut self, observation: ScreenObservation<'_>) -> bool {
        match observation {
            ScreenObservation::Busy => {
                self.completion_before_work = None;
                false
            }
            ScreenObservation::NeedsAttention(completion) => {
                if self.completion_before_work.as_deref() == Some(completion) {
                    return false;
                }
                self.completion_before_work = None;
                true
            }
            // Partial redraws must not make the previous turn's completion fresh again.
            ScreenObservation::Unknown => false,
        }
    }

    fn quiet_activity_at(&self, focus_state: TrackedProcessPaneFocus) -> Instant {
        if focus_state == TrackedProcessPaneFocus::Unfocused {
            return self.last_tracked_activity;
        }

        // Focused user input keeps a busy indicator alive, while prompt submit or output anchors quiet clearing for
        // both focused and unfocused panes.
        self.last_focused_user_interaction
            .map_or(self.last_tracked_activity, |activity| {
                self.last_tracked_activity.max(activity)
            })
    }

    const fn mark_visible_activity(&mut self) -> TrackedProcessStateChange {
        match self.status {
            PaneTrackedProcessStatus::Busy => TrackedProcessStateChange::Unchanged,
            PaneTrackedProcessStatus::Seen | PaneTrackedProcessStatus::Unseen | PaneTrackedProcessStatus::Settling => {
                self.status = PaneTrackedProcessStatus::Busy;
                TrackedProcessStateChange::Changed
            }
        }
    }

    fn mark_quiet(&mut self, quiet_for: Duration, focus_state: TrackedProcessPaneFocus) -> TrackedProcessStateChange {
        if !self.has_quiet_deadline() {
            return TrackedProcessStateChange::Unchanged;
        }
        if quiet_for < self.tracked_process.quiet_threshold {
            return TrackedProcessStateChange::Unchanged;
        }

        self.status = match focus_state {
            TrackedProcessPaneFocus::Focused => PaneTrackedProcessStatus::Seen,
            TrackedProcessPaneFocus::Unfocused => PaneTrackedProcessStatus::Unseen,
        };
        TrackedProcessStateChange::Changed
    }

    const fn has_quiet_deadline(&self) -> bool {
        matches!(
            self.status,
            PaneTrackedProcessStatus::Busy | PaneTrackedProcessStatus::Settling
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TrackedProcessPaneFocus {
    Focused,
    Unfocused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TrackedProcessAttentionNeed {
    NeedsAttention,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneTrackedProcessStatus {
    Busy,
    // A completion was observed without a Working row. Wait for the quiet threshold without showing green.
    Settling,
    Seen,
    Unseen,
}

impl From<PaneTrackedProcessStatus> for TrackedProcessState {
    fn from(status: PaneTrackedProcessStatus) -> Self {
        match status {
            PaneTrackedProcessStatus::Busy => Self::Busy,
            PaneTrackedProcessStatus::Seen | PaneTrackedProcessStatus::Settling => Self::Seen,
            PaneTrackedProcessStatus::Unseen => Self::Unseen,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingTrackedWorkStart {
    None,
    Pending,
}
