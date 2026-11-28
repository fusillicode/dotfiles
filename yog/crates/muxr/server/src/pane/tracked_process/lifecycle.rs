use std::time::Duration;
use std::time::Instant;

use muxr_config::ScreenObservationConfig;
use muxr_config::TrackedProcess;
use muxr_core::TrackedProcessState;

use crate::pane::tracked_process::ScreenSampleSource;
use crate::pane::tracked_process::TrackedProcessChanges;
use crate::pane::tracked_process::TrackedProcessStateChange;
use crate::pane::tracked_process::TrackedProcessUserInteraction;
use crate::pane::tracked_process::screen;
use crate::pane::tracked_process::screen::BusyStart;
use crate::pane::tracked_process::screen::ScreenObservation;
use crate::pane::tracked_process::screen::ScreenObserver;
use crate::terminal::TerminalTextTail;

#[cfg(test)]
mod tests;

// Each Codex keystroke restarts this window so prompt edits alone leave an idle pane neutral.
// A short reply can therefore appear without turning the dot green. A recognized Working banner bypasses the window.
const USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub(super) struct PaneTrackedProcessLifecycle {
    completion_before_work: Option<String>,
    last_focused_user_interaction: Option<Instant>,
    last_sampled_dmg: Option<u64>,
    last_tracked_activity: Instant,
    pending_work_start: PendingTrackedWorkStart,
    recent_user_interaction: Option<Instant>,
    screen_observer: ScreenObserver,
    status: PaneTrackedProcessStatus,
    tracked_process: TrackedProcess,
}

impl PaneTrackedProcessLifecycle {
    pub(super) const fn new(tracked_process: TrackedProcess, now: Instant) -> Self {
        let status = match screen::busy_start(&tracked_process) {
            BusyStart::Activity => PaneTrackedProcessStatus::Busy,
            BusyStart::ScreenOutput => PaneTrackedProcessStatus::Seen,
        };
        Self {
            completion_before_work: None,
            last_focused_user_interaction: None,
            last_sampled_dmg: None,
            last_tracked_activity: now,
            pending_work_start: PendingTrackedWorkStart::None,
            recent_user_interaction: None,
            screen_observer: ScreenObserver::new(),
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
        // For Codex, Enter can redraw an empty prompt just like any other edit.
        if interaction == TrackedProcessUserInteraction::MayEcho || self.busy_start() == BusyStart::ScreenOutput {
            self.recent_user_interaction = Some(now);
            return if focused_deadline_extended {
                TrackedProcessChanges::deadline_only()
            } else {
                TrackedProcessChanges::default()
            };
        }

        self.recent_user_interaction = None;
        self.pending_work_start = PendingTrackedWorkStart::Pending;
        self.last_tracked_activity = now;
        TrackedProcessChanges::for_activity(self.mark_visible_activity())
    }

    pub(super) fn record_screen_activity(
        &mut self,
        observation: ScreenObservation<'_>,
        now: Instant,
    ) -> TrackedProcessChanges {
        match observation {
            ScreenObservation::Cancelled => self.cancel_work(),
            ScreenObservation::Busy => {
                // Positive work evidence takes precedence over local-echo suppression and needs no Enter event.
                self.clear_pending_work_start();
                self.last_tracked_activity = now;
                TrackedProcessChanges::for_activity(self.mark_visible_activity())
            }
            ScreenObservation::NeedsAttention(completion) if self.accept_completion(completion) => {
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
        self.mark_output_activity(now, now)
    }

    fn mark_output_activity(&mut self, arrived_at: Instant, processed_at: Instant) -> TrackedProcessChanges {
        if self.input_may_echo_at(arrived_at) {
            // Typing and mouse gestures can cause the application to repaint.
            return TrackedProcessChanges::default();
        }
        if self.busy_start() == BusyStart::Activity
            && self.status != PaneTrackedProcessStatus::Busy
            && self.pending_work_start == PendingTrackedWorkStart::None
        {
            // Submit-driven agents can repaint idle UI. Codex can resume streaming without a banner or a submit.
            return TrackedProcessChanges::default();
        }

        self.pending_work_start = PendingTrackedWorkStart::None;
        self.last_tracked_activity = processed_at;
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

    fn input_may_echo_at(&self, arrived_at: Instant) -> bool {
        self.recent_user_interaction
            .and_then(|input| arrived_at.checked_duration_since(input))
            .is_some_and(|elapsed| elapsed <= USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION)
    }

    pub(super) const fn last_sampled_dmg(&self) -> Option<u64> {
        self.last_sampled_dmg
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

    pub(super) fn record_screen_sample(
        &mut self,
        text: &TerminalTextTail,
        source: ScreenSampleSource,
        now: Instant,
    ) -> TrackedProcessChanges {
        let codex_dmg = text.dmg.filter(|_| self.busy_start() == BusyStart::ScreenOutput);
        if source == ScreenSampleSource::TerminalDmg
            && codex_dmg.is_some_and(|dmg| self.last_sampled_dmg == Some(dmg.generation))
        {
            return TrackedProcessChanges::default();
        }
        let observation = self.observe_screen(text);
        if !matches!(observation, ScreenObservation::Unknown) {
            return self.record_screen_activity(observation, now);
        }
        if source == ScreenSampleSource::ProcessDiscovery {
            return TrackedProcessChanges::default();
        }
        if let Some(dmg) = codex_dmg {
            self.last_sampled_dmg = Some(dmg.generation);
            return self.mark_output_activity(dmg.arrived_at, now);
        }
        self.record_visible_activity(now)
    }

    pub(super) fn observe_screen<'a>(&mut self, text: &'a TerminalTextTail) -> ScreenObservation<'a> {
        let Some(patterns) = self.tracked_process.screen_observation.as_ref() else {
            return ScreenObservation::Unknown;
        };
        let observation = self.screen_observer.observe(
            patterns,
            text.candidate_line_spans(),
            self.busy_start(),
            text.screen_mode,
        );
        if !matches!(observation, ScreenObservation::Unknown) {
            self.last_sampled_dmg = text.dmg.map(|dmg| dmg.generation);
        }
        observation
    }

    pub(super) const fn busy_start(&self) -> BusyStart {
        screen::busy_start(&self.tracked_process)
    }

    pub(super) const fn captures_completion_before_work(&self) -> bool {
        matches!(self.busy_start(), BusyStart::Activity) && self.screen_observation().is_some()
    }

    pub(super) fn capture_completion_before_work(&mut self, observation: ScreenObservation<'_>) {
        match observation {
            ScreenObservation::NeedsAttention(completion) => {
                self.completion_before_work = Some(completion.to_owned());
            }
            ScreenObservation::Busy | ScreenObservation::Cancelled => self.completion_before_work = None,
            ScreenObservation::Unknown => {}
        }
    }

    pub(super) fn guard_quiet_deadline(
        &mut self,
        observation: Option<ScreenObservation<'_>>,
        now: Instant,
    ) -> TrackedProcessChanges {
        match observation {
            Some(ScreenObservation::Cancelled) => self.cancel_work(),
            Some(ScreenObservation::Busy) => self.record_screen_activity(ScreenObservation::Busy, now),
            Some(ScreenObservation::Unknown) if self.busy_start() == BusyStart::ScreenOutput => {
                TrackedProcessChanges::default()
            }
            Some(ScreenObservation::NeedsAttention(completion)) if self.accept_completion(completion) => {
                TrackedProcessChanges::default()
            }
            Some(ScreenObservation::NeedsAttention(_) | ScreenObservation::Unknown) | None => {
                self.extend_quiet_deadline(now)
            }
        }
    }

    const fn extend_quiet_deadline(&mut self, now: Instant) -> TrackedProcessChanges {
        self.last_tracked_activity = now;
        TrackedProcessChanges::deadline_only()
    }

    fn cancel_work(&mut self) -> TrackedProcessChanges {
        let changed = self.status != PaneTrackedProcessStatus::Seen;
        self.status = PaneTrackedProcessStatus::Seen;
        self.clear_pending_work_start();
        self.last_focused_user_interaction = None;
        if changed {
            TrackedProcessChanges::state_and_deadline()
        } else {
            TrackedProcessChanges::default()
        }
    }

    fn clear_pending_work_start(&mut self) {
        self.pending_work_start = PendingTrackedWorkStart::None;
        self.completion_before_work = None;
        // Codex prompt echo can arrive after a status change. Its window still belongs to the last input.
        if matches!(self.busy_start(), BusyStart::Activity) {
            self.recent_user_interaction = None;
        }
    }

    fn accept_completion(&mut self, completion: &str) -> bool {
        if self.completion_before_work.as_deref() == Some(completion) {
            return false;
        }
        self.completion_before_work = None;
        true
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
    // The footer-based path can settle a pending submission without showing green.
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
