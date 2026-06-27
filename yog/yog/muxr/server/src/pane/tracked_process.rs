use std::collections::BTreeMap;
use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use muxr_config::MuxrConfig;
use muxr_config::TrackedProcess;
use muxr_core::PaneId;
use muxr_core::TrackedProcessState;

use crate::pane::cmd::FgCmd;
use crate::pane::cmd::PaneCmdObservation;
use crate::pane::cmd::PaneCmdSnapshot;
use crate::pane::cmd::ProcessGroupLookupError;
use crate::pane::runtime::PaneRuntimes;
use crate::state::SessionLayout;

const USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION: Duration = Duration::from_millis(500);

#[derive(Debug, Default)]
pub struct PaneTrackedProcesses {
    by_pane: HashMap<PaneId, PaneTrackedProcessLifecycle>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrackedProcessAttention {
    Seen,
    Unchanged,
    Unseen { pane_ids: Vec<PaneId> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneTrackedProcessSnapshotEntry {
    label: String,
    state: TrackedProcessState,
}

impl PaneTrackedProcessSnapshotEntry {
    pub fn label(&self) -> &str {
        &self.label
    }

    pub const fn state(&self) -> TrackedProcessState {
        self.state
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaneTrackedProcessSnapshot {
    panes: BTreeMap<PaneId, PaneTrackedProcessSnapshotEntry>,
}

impl PaneTrackedProcessSnapshot {
    pub fn panes(&self) -> impl Iterator<Item = (PaneId, &PaneTrackedProcessSnapshotEntry)> {
        self.panes.iter().map(|(pane_id, pane)| (*pane_id, pane))
    }
}

// Keep effect construction centralized so a sidebar state change cannot be reported without the matching quiet-deadline
// resync. Callers can merge/read effects, but only this module creates non-empty combinations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TrackedProcessChanges {
    change: TrackedProcessChange,
}

// Client-origin tracked-process changes must carry the pane id needed for sidebar/layout updates; keep them paired so
// callers cannot sync the timer and accidentally skip a visible state update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackedProcessClientChange {
    changes: TrackedProcessChanges,
    pane_id: PaneId,
}

impl TrackedProcessClientChange {
    fn from_changes(pane_id: PaneId, changes: TrackedProcessChanges) -> Option<Self> {
        (changes.presence() == TrackedProcessChangePresence::Present).then_some(Self { changes, pane_id })
    }

    pub const fn changes(self) -> TrackedProcessChanges {
        self.changes
    }

    pub const fn pane_id(self) -> PaneId {
        self.pane_id
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TrackedProcessChange {
    #[default]
    None,
    Deadline,
    State,
}

impl TrackedProcessChanges {
    const fn deadline_only() -> Self {
        Self {
            change: TrackedProcessChange::Deadline,
        }
    }

    const fn state_and_deadline() -> Self {
        Self {
            change: TrackedProcessChange::State,
        }
    }

    pub const fn deadline_change(self) -> TrackedProcessDeadlineChange {
        match self.change {
            TrackedProcessChange::Deadline | TrackedProcessChange::State => TrackedProcessDeadlineChange::Changed,
            TrackedProcessChange::None => TrackedProcessDeadlineChange::Unchanged,
        }
    }

    pub const fn state_change(self) -> TrackedProcessStateChange {
        match self.change {
            TrackedProcessChange::State => TrackedProcessStateChange::Changed,
            TrackedProcessChange::Deadline | TrackedProcessChange::None => TrackedProcessStateChange::Unchanged,
        }
    }

    pub const fn presence(self) -> TrackedProcessChangePresence {
        match self.change {
            TrackedProcessChange::Deadline | TrackedProcessChange::State => TrackedProcessChangePresence::Present,
            TrackedProcessChange::None => TrackedProcessChangePresence::Empty,
        }
    }

    const fn include_state_change(&mut self) {
        self.change = TrackedProcessChange::State;
    }

    const fn merge(&mut self, other: Self) {
        self.change = self.change.merge(other.change);
    }

    const fn for_activity(state_change: TrackedProcessStateChange) -> Self {
        match state_change {
            TrackedProcessStateChange::Changed => Self::state_and_deadline(),
            TrackedProcessStateChange::Unchanged => Self::deadline_only(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackedProcessChangePresence {
    Empty,
    Present,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackedProcessDeadlineChange {
    Changed,
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackedProcessStateChange {
    Changed,
    Unchanged,
}

impl TrackedProcessChange {
    const fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::State, _) | (_, Self::State) => Self::State,
            (Self::Deadline, _) | (_, Self::Deadline) => Self::Deadline,
            (Self::None, Self::None) => Self::None,
        }
    }
}

#[derive(Debug)]
struct PaneTrackedProcessLifecycle {
    last_focused_user_interaction: Option<Instant>,
    last_tracked_activity: Instant,
    pending_work_start: PendingTrackedWorkStart,
    recent_user_interaction: Option<Instant>,
    status: PaneTrackedProcessStatus,
    tracked_process: TrackedProcess,
}

// Observations borrow the read-only config entry so hot visible-activity samples do not clone matcher Vecs. The
// lifecycle clones only when it actually stores a newly tracked process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrackedProcessCmdObservation<'a> {
    Tracked(&'a TrackedProcess),
    TrustedUntracked,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneTrackedProcessStatus {
    Busy,
    Seen,
    Unseen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingTrackedWorkStart {
    None,
    Pending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrackedProcessPaneFocus {
    Focused,
    Unfocused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrackedProcessAttentionNeed {
    NeedsAttention,
    None,
}

impl From<PaneTrackedProcessStatus> for TrackedProcessState {
    fn from(status: PaneTrackedProcessStatus) -> Self {
        match status {
            PaneTrackedProcessStatus::Busy => Self::Busy,
            PaneTrackedProcessStatus::Seen => Self::Seen,
            PaneTrackedProcessStatus::Unseen => Self::Unseen,
        }
    }
}

impl PaneTrackedProcessLifecycle {
    const fn new(tracked_process: TrackedProcess, now: Instant) -> Self {
        Self {
            last_focused_user_interaction: None,
            last_tracked_activity: now,
            pending_work_start: PendingTrackedWorkStart::None,
            recent_user_interaction: None,
            status: PaneTrackedProcessStatus::Busy,
            tracked_process,
        }
    }

    fn observe_tracked_process(&mut self, tracked_process: &TrackedProcess, now: Instant) -> TrackedProcessStateChange {
        if self.tracked_process.id == tracked_process.id {
            return TrackedProcessStateChange::Unchanged;
        }

        // A different tracked foreground process starts a new lifecycle; old activity, attention, and local-echo
        // suppression belonged to the previous process and must not carry over.
        *self = Self::new(tracked_process.clone(), now);
        TrackedProcessStateChange::Changed
    }

    fn record_user_interaction(
        &mut self,
        interaction: TrackedProcessUserInteraction,
        now: Instant,
        focus_state: TrackedProcessPaneFocus,
    ) -> TrackedProcessChanges {
        // Focused local echo does not change sidebar state, but it can still extend the quiet deadline for Busy.
        let focused_deadline_extended = focus_state == TrackedProcessPaneFocus::Focused
            && self.status == PaneTrackedProcessStatus::Busy
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
                // Prompt submit starts tracked work even before output. Use it as the quiet-deadline anchor so a
                // silent agent turn still shows Busy, then clears after the configured quiet threshold.
                self.pending_work_start = PendingTrackedWorkStart::Pending;
                self.last_tracked_activity = now;
                self.recent_user_interaction = None;
                TrackedProcessChanges::for_activity(self.mark_visible_activity())
            }
        }
    }

    fn record_visible_activity(&mut self, tracked_process: &TrackedProcess, now: Instant) -> TrackedProcessChanges {
        if self.tracked_process.id != tracked_process.id {
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

    fn mark_quiet_if_due(&mut self, now: Instant, focus_state: TrackedProcessPaneFocus) -> TrackedProcessStateChange {
        self.mark_quiet(
            now.saturating_duration_since(self.quiet_activity_at(focus_state)),
            focus_state,
        )
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
            PaneTrackedProcessStatus::Seen | PaneTrackedProcessStatus::Unseen => {
                self.status = PaneTrackedProcessStatus::Busy;
                TrackedProcessStateChange::Changed
            }
        }
    }

    fn mark_quiet(&mut self, quiet_for: Duration, focus_state: TrackedProcessPaneFocus) -> TrackedProcessStateChange {
        if self.status != PaneTrackedProcessStatus::Busy {
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

    const fn attention_need(&self) -> TrackedProcessAttentionNeed {
        match self.status {
            PaneTrackedProcessStatus::Unseen => TrackedProcessAttentionNeed::NeedsAttention,
            PaneTrackedProcessStatus::Busy | PaneTrackedProcessStatus::Seen => TrackedProcessAttentionNeed::None,
        }
    }

    const fn state(&self) -> TrackedProcessState {
        match self.status {
            PaneTrackedProcessStatus::Busy => TrackedProcessState::Busy,
            PaneTrackedProcessStatus::Seen => TrackedProcessState::Seen,
            PaneTrackedProcessStatus::Unseen => TrackedProcessState::Unseen,
        }
    }

    const fn acknowledge_attention(&mut self) -> TrackedProcessStateChange {
        if !matches!(self.attention_need(), TrackedProcessAttentionNeed::NeedsAttention) {
            return TrackedProcessStateChange::Unchanged;
        }
        self.status = PaneTrackedProcessStatus::Seen;
        TrackedProcessStateChange::Changed
    }

    fn discard_stale_user_interaction(&mut self, now: Instant) {
        let Some(last_activity) = self.recent_user_interaction else {
            return;
        };
        if now.saturating_duration_since(last_activity) > USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION {
            self.recent_user_interaction = None;
        }
    }

    fn quiet_deadline(&self, focus_state: TrackedProcessPaneFocus) -> rootcause::Result<Option<Instant>> {
        if self.status != PaneTrackedProcessStatus::Busy {
            return Ok(None);
        }
        self.quiet_activity_at(focus_state)
            .checked_add(self.tracked_process.quiet_threshold)
            .map(Some)
            .ok_or_else(|| rootcause::report!("muxr tracked-process quiet deadline overflowed"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackedProcessUserInteraction {
    MayEcho,
    StartsTrackedProcessWork,
}

impl PaneTrackedProcesses {
    pub fn observe_all_runtime_pane_cmds(
        &mut self,
        config: &MuxrConfig,
        layout: &SessionLayout,
        runtimes: &PaneRuntimes,
        now: Instant,
    ) -> rootcause::Result<TrackedProcessChanges> {
        let pane_ids = layout.pane_ids();
        self.observe_runtime_pane_cmds(config, runtimes, &pane_ids, now)
    }

    pub fn observe_runtime_pane_cmds(
        &mut self,
        config: &MuxrConfig,
        runtimes: &PaneRuntimes,
        pane_ids: &[PaneId],
        now: Instant,
    ) -> rootcause::Result<TrackedProcessChanges> {
        let mut changes = TrackedProcessChanges::default();
        for pane_id in pane_ids {
            let observation = self::runtime_pane_cmd_observation(runtimes, *pane_id)?;
            changes.merge(self.observe_pane_cmd(config, *pane_id, &observation, now));
        }
        Ok(changes)
    }

    pub fn observe_runtime_visible_activity(
        &mut self,
        config: &MuxrConfig,
        runtimes: &PaneRuntimes,
        pane_ids: &[PaneId],
        now: Instant,
    ) -> rootcause::Result<TrackedProcessChanges> {
        let mut changes = TrackedProcessChanges::default();
        for pane_id in pane_ids {
            let observation = self::runtime_pane_cmd_observation(runtimes, *pane_id)?;
            let pane_changes = self.observe_visible_activity(config, *pane_id, &observation, now);
            changes.merge(pane_changes);
        }
        Ok(changes)
    }

    pub fn acknowledge_active_pane_attention(
        &mut self,
        config: &MuxrConfig,
        layout: &SessionLayout,
        runtimes: &PaneRuntimes,
        now: Instant,
    ) -> rootcause::Result<TrackedProcessChanges> {
        let active_pane = layout.active_pane_id()?;
        let observation = self::runtime_pane_cmd_observation(runtimes, active_pane)?;
        let mut changes = self.observe_pane_cmd(config, active_pane, &observation, now);
        changes.merge(self.acknowledge_attention(active_pane));
        Ok(changes)
    }

    pub fn observe_pane_cmd(
        &mut self,
        config: &MuxrConfig,
        pane_id: PaneId,
        observation: &PaneCmdObservation,
        now: Instant,
    ) -> TrackedProcessChanges {
        if self.apply_cmd_observation(
            pane_id,
            self::tracked_process_observation_from_pane_cmd(config, observation),
            now,
        ) == TrackedProcessStateChange::Changed
        {
            TrackedProcessChanges::state_and_deadline()
        } else {
            TrackedProcessChanges::default()
        }
    }

    pub fn observe_visible_activity(
        &mut self,
        config: &MuxrConfig,
        pane_id: PaneId,
        observation: &PaneCmdObservation,
        now: Instant,
    ) -> TrackedProcessChanges {
        let cmd_observation = self::tracked_process_observation_from_pane_cmd(config, observation);
        let tracked_process = match cmd_observation {
            TrackedProcessCmdObservation::Tracked(tracked_process) => Some(tracked_process),
            TrackedProcessCmdObservation::TrustedUntracked | TrackedProcessCmdObservation::Unknown => None,
        };
        let state_change = self.apply_cmd_observation(pane_id, cmd_observation, now);
        let activity_changes = self.record_visible_activity(pane_id, tracked_process, now);
        let mut changes = activity_changes;
        if state_change == TrackedProcessStateChange::Changed {
            changes.include_state_change();
        }
        changes
    }

    pub fn mark_quiet_deadlines(
        &mut self,
        layout: &SessionLayout,
        now: Instant,
    ) -> rootcause::Result<TrackedProcessAttention> {
        // Pane removal owns pruning. Quiet sweeps use the supplied layout only for focus/visibility transitions because
        // attached-client-local layouts, such as scrollback editor mode, can temporarily hide real panes.
        self.discard_stale_user_interactions(now);
        self.mark_quiet_tracked_processes(layout, now)
    }

    pub fn attention_pane_ids(&self, layout: &SessionLayout) -> Vec<PaneId> {
        let mut pane_ids = Vec::new();
        layout.for_each_pane_id(|pane_id| {
            if self.attention_need(pane_id) == TrackedProcessAttentionNeed::NeedsAttention {
                pane_ids.push(pane_id);
            }
        });
        pane_ids
    }

    pub fn next_quiet_deadline(&self, layout: &SessionLayout) -> rootcause::Result<Option<Instant>> {
        // Layout-scoped reads project tracked state onto panes the attached client can address; pane removal owns
        // pruning, so temporary attached-client layouts must not delete hidden real-pane lifecycles.
        if self.by_pane.is_empty() {
            return Ok(None);
        }
        let focused_pane = layout.active_pane_id()?;
        let mut deadline = None;
        let mut error = None;
        layout.for_each_pane_id(|pane_id| {
            if error.is_some() {
                return;
            }
            let Some(pane_tracked_process) = self.by_pane.get(&pane_id) else {
                return;
            };
            let focus_state = if pane_id == focused_pane {
                TrackedProcessPaneFocus::Focused
            } else {
                TrackedProcessPaneFocus::Unfocused
            };
            let pane_deadline = match pane_tracked_process.quiet_deadline(focus_state) {
                Ok(Some(pane_deadline)) => pane_deadline,
                Ok(None) => return,
                Err(deadline_error) => {
                    error = Some(deadline_error);
                    return;
                }
            };
            deadline = Some(deadline.map_or(pane_deadline, |current: Instant| current.min(pane_deadline)));
        });
        if let Some(error) = error {
            return Err(error);
        }
        Ok(deadline)
    }

    pub fn snapshot(&self, layout: &SessionLayout) -> PaneTrackedProcessSnapshot {
        // See `next_quiet_deadline`: snapshots use layout for projection only, not as the tracked-state owner.
        if self.by_pane.is_empty() {
            return PaneTrackedProcessSnapshot::default();
        }
        let mut panes = BTreeMap::new();
        layout.for_each_pane_id(|pane_id| {
            let Some(pane_tracked_process) = self.by_pane.get(&pane_id) else {
                return;
            };
            panes.insert(
                pane_id,
                PaneTrackedProcessSnapshotEntry {
                    label: pane_tracked_process.tracked_process.label.to_owned(),
                    state: pane_tracked_process.state(),
                },
            );
        });
        PaneTrackedProcessSnapshot { panes }
    }

    pub fn remove_pane(&mut self, pane_id: PaneId) -> TrackedProcessChanges {
        if self.by_pane.remove(&pane_id).is_some() {
            TrackedProcessChanges::state_and_deadline()
        } else {
            TrackedProcessChanges::default()
        }
    }

    fn mark_quiet_tracked_processes(
        &mut self,
        layout: &SessionLayout,
        now: Instant,
    ) -> rootcause::Result<TrackedProcessAttention> {
        let focused_pane = layout.active_pane_id()?;
        let mut seen = false;
        let mut unseen_panes = Vec::new();
        layout.for_each_pane_id(|pane_id| {
            let focus_state = if pane_id == focused_pane {
                TrackedProcessPaneFocus::Focused
            } else {
                TrackedProcessPaneFocus::Unfocused
            };
            if self.mark_quiet_if_due(pane_id, now, focus_state) == TrackedProcessStateChange::Changed {
                // The state machine owns first-time attention transitions; callers should react to this outcome instead
                // of diffing snapshots and duplicating status rules outside this feature.
                if focus_state == TrackedProcessPaneFocus::Focused {
                    seen = true;
                } else if self.attention_need(pane_id) == TrackedProcessAttentionNeed::NeedsAttention {
                    unseen_panes.push(pane_id);
                }
            }
        });
        if !unseen_panes.is_empty() {
            Ok(TrackedProcessAttention::Unseen { pane_ids: unseen_panes })
        } else if seen {
            Ok(TrackedProcessAttention::Seen)
        } else {
            Ok(TrackedProcessAttention::Unchanged)
        }
    }

    fn apply_cmd_observation(
        &mut self,
        pane_id: PaneId,
        observation: TrackedProcessCmdObservation<'_>,
        now: Instant,
    ) -> TrackedProcessStateChange {
        match observation {
            TrackedProcessCmdObservation::Tracked(tracked_process) => {
                self.observe_tracked_process(pane_id, tracked_process, now)
            }
            TrackedProcessCmdObservation::TrustedUntracked => {
                // A trusted shell/untracked observation ends the current lifecycle. Unknown observations do not.
                if self.by_pane.remove(&pane_id).is_some() {
                    TrackedProcessStateChange::Changed
                } else {
                    TrackedProcessStateChange::Unchanged
                }
            }
            TrackedProcessCmdObservation::Unknown => TrackedProcessStateChange::Unchanged,
        }
    }

    fn observe_tracked_process(
        &mut self,
        pane_id: PaneId,
        tracked_process: &TrackedProcess,
        now: Instant,
    ) -> TrackedProcessStateChange {
        let Some(lifecycle) = self.by_pane.get_mut(&pane_id) else {
            // A newly observed tracked process starts Busy even if its first dirty frame was missed by the sampled
            // scan. Pre-process input suppression belongs to the shell and must not hide the new process output.
            self.by_pane
                .insert(pane_id, PaneTrackedProcessLifecycle::new(tracked_process.clone(), now));
            return TrackedProcessStateChange::Changed;
        };
        lifecycle.observe_tracked_process(tracked_process, now)
    }

    pub fn record_user_interaction(
        &mut self,
        layout: &SessionLayout,
        pane_id: PaneId,
        interaction: TrackedProcessUserInteraction,
        now: Instant,
    ) -> rootcause::Result<TrackedProcessChanges> {
        if let Some(lifecycle) = self.by_pane.get_mut(&pane_id) {
            let focus_state = if pane_id == layout.active_pane_id()? {
                TrackedProcessPaneFocus::Focused
            } else {
                TrackedProcessPaneFocus::Unfocused
            };
            return Ok(lifecycle.record_user_interaction(interaction, now, focus_state));
        }
        Ok(TrackedProcessChanges::default())
    }

    pub fn record_client_user_interaction(
        &mut self,
        layout: &SessionLayout,
        pane_id: PaneId,
        interaction: TrackedProcessUserInteraction,
        now: Instant,
    ) -> rootcause::Result<Option<TrackedProcessClientChange>> {
        let changes = self.record_user_interaction(layout, pane_id, interaction, now)?;
        Ok(TrackedProcessClientChange::from_changes(pane_id, changes))
    }

    // Active-pane input resolves the focused pane handle in the same turn before calling this.
    // Other client events must use the layout-aware API so focus cannot be asserted for an arbitrary pane.
    pub(in crate::pane) fn record_focused_client_user_interaction(
        &mut self,
        pane_id: PaneId,
        interaction: TrackedProcessUserInteraction,
        now: Instant,
    ) -> Option<TrackedProcessClientChange> {
        let lifecycle = self.by_pane.get_mut(&pane_id)?;
        let changes = lifecycle.record_user_interaction(interaction, now, TrackedProcessPaneFocus::Focused);
        TrackedProcessClientChange::from_changes(pane_id, changes)
    }

    fn record_visible_activity(
        &mut self,
        pane_id: PaneId,
        tracked_process: Option<&TrackedProcess>,
        now: Instant,
    ) -> TrackedProcessChanges {
        let Some(tracked_process) = tracked_process else {
            // PTY output before process detection is still rendered, but it is not tracked-process activity yet.
            // Newly detected tracked processes start Busy instead of inheriting stale shell output.
            return TrackedProcessChanges::default();
        };
        let Some(pane_tracked_process) = self.by_pane.get_mut(&pane_id) else {
            return TrackedProcessChanges::default();
        };
        pane_tracked_process.record_visible_activity(tracked_process, now)
    }

    fn mark_quiet_if_due(
        &mut self,
        pane_id: PaneId,
        now: Instant,
        focus_state: TrackedProcessPaneFocus,
    ) -> TrackedProcessStateChange {
        let Some(pane_tracked_process) = self.by_pane.get_mut(&pane_id) else {
            return TrackedProcessStateChange::Unchanged;
        };
        pane_tracked_process.mark_quiet_if_due(now, focus_state)
    }

    pub fn acknowledge_attention(&mut self, pane_id: PaneId) -> TrackedProcessChanges {
        if self
            .by_pane
            .get_mut(&pane_id)
            .is_some_and(|lifecycle| lifecycle.acknowledge_attention() == TrackedProcessStateChange::Changed)
        {
            TrackedProcessChanges::state_and_deadline()
        } else {
            TrackedProcessChanges::default()
        }
    }

    fn attention_need(&self, pane_id: PaneId) -> TrackedProcessAttentionNeed {
        self.by_pane.get(&pane_id).map_or(
            TrackedProcessAttentionNeed::None,
            PaneTrackedProcessLifecycle::attention_need,
        )
    }

    fn discard_stale_user_interactions(&mut self, now: Instant) {
        for pane_tracked_process in self.by_pane.values_mut() {
            pane_tracked_process.discard_stale_user_interaction(now);
        }
    }
}

fn tracked_process_observation_from_pane_cmd<'a>(
    config: &'a MuxrConfig,
    observation: &PaneCmdObservation,
) -> TrackedProcessCmdObservation<'a> {
    match observation {
        PaneCmdObservation::FgCmd(fg_cmd) => self::tracked_process_from_fg_cmd(config, fg_cmd),
        PaneCmdObservation::Shell => TrackedProcessCmdObservation::TrustedUntracked,
        PaneCmdObservation::Unknown { .. } => TrackedProcessCmdObservation::Unknown,
    }
}

fn tracked_process_from_fg_cmd<'a>(config: &'a MuxrConfig, fg_cmd: &FgCmd) -> TrackedProcessCmdObservation<'a> {
    let leader_cmd = fg_cmd.leader_cmd();
    if let Some(cmd) = leader_cmd
        && let Some(tracked_process) = config.tracked_process_for_cmd(&cmd.executable, cmd.path.as_deref())
    {
        return TrackedProcessCmdObservation::Tracked(tracked_process);
    }

    match fg_cmd.process_group_cmds() {
        Ok(process_group_cmds) => {
            let tracked_process = process_group_cmds
                .iter()
                .find_map(|cmd| config.tracked_process_for_cmd(&cmd.executable, cmd.path.as_deref()));
            match (tracked_process, leader_cmd) {
                (Some(tracked_process), _) => TrackedProcessCmdObservation::Tracked(tracked_process),
                (None, Some(_)) => TrackedProcessCmdObservation::TrustedUntracked,
                (None, None) => TrackedProcessCmdObservation::Unknown,
            }
        }
        Err(ProcessGroupLookupError::Failed) => TrackedProcessCmdObservation::Unknown,
    }
}

fn runtime_pane_cmd_observation(runtimes: &PaneRuntimes, pane_id: PaneId) -> rootcause::Result<PaneCmdObservation> {
    let handle = runtimes.handle(pane_id)?;
    let snapshot = PaneCmdSnapshot::try_from(&handle)?;
    Ok(PaneCmdObservation::from(&snapshot))
}

#[cfg(test)]
mod tests {
    use muxr_core::SessionName;

    use super::*;
    use crate::pane::cmd::PaneCmd;
    use crate::pane::cmd::PaneCmdUnknownReason;
    use crate::pane::split::PaneSplitAxis;
    use crate::state::SessionMetadata;

    fn tracked_process(executable: &str) -> rootcause::Result<TrackedProcess> {
        MuxrConfig::default()
            .tracked_process_for_cmd(executable, None)
            .cloned()
            .ok_or_else(|| rootcause::report!("expected configured tracked process"))
    }

    fn pane_tracked_process_status(
        pane_tracked_processes: &PaneTrackedProcesses,
        pane_id: PaneId,
    ) -> TrackedProcessState {
        pane_tracked_processes
            .by_pane
            .get(&pane_id)
            .map_or(TrackedProcessState::None, PaneTrackedProcessLifecycle::state)
    }

    fn set_pane_tracked_process_status(
        pane_tracked_processes: &mut PaneTrackedProcesses,
        pane_id: PaneId,
        status: PaneTrackedProcessStatus,
    ) {
        if let Some(pane_tracked_process) = pane_tracked_processes.by_pane.get_mut(&pane_id) {
            pane_tracked_process.status = status;
        }
    }

    #[test]
    fn test_pane_tracked_process_lifecycle_when_created_starts_busy() -> rootcause::Result<()> {
        let pane_tracked_process = PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, Instant::now());

        pretty_assertions::assert_eq!(pane_tracked_process.state(), TrackedProcessState::Busy);
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
        let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, then);
        pane_tracked_process.status = starting_status;
        pane_tracked_process.record_user_interaction(
            TrackedProcessUserInteraction::MayEcho,
            then,
            TrackedProcessPaneFocus::Focused,
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process.record_visible_activity(
                &self::tracked_process("codex")?,
                self::instant_after(then, Duration::from_millis(100))?,
            ),
            TrackedProcessChanges::default()
        );

        pretty_assertions::assert_eq!(pane_tracked_process.state(), expected_state);
        Ok(())
    }

    #[rstest::rstest]
    #[case::seen(PaneTrackedProcessStatus::Seen)]
    #[case::unseen(PaneTrackedProcessStatus::Unseen)]
    fn test_pane_tracked_process_lifecycle_when_prompt_submit_without_output_marks_busy(
        #[case] starting_status: PaneTrackedProcessStatus,
    ) -> rootcause::Result<()> {
        let then = Instant::now();
        let prompt_submitted_at = self::instant_after(then, Duration::from_millis(100))?;
        let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, then);
        pane_tracked_process.status = starting_status;
        pane_tracked_process.record_user_interaction(
            TrackedProcessUserInteraction::MayEcho,
            then,
            TrackedProcessPaneFocus::Focused,
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process.record_user_interaction(
                TrackedProcessUserInteraction::StartsTrackedProcessWork,
                prompt_submitted_at,
                TrackedProcessPaneFocus::Focused,
            ),
            TrackedProcessChanges::state_and_deadline()
        );

        pretty_assertions::assert_eq!(pane_tracked_process.state(), TrackedProcessState::Busy);
        pretty_assertions::assert_eq!(
            pane_tracked_process.quiet_deadline(TrackedProcessPaneFocus::Focused)?,
            Some(self::instant_after(prompt_submitted_at, Duration::from_secs(3))?)
        );
        Ok(())
    }

    #[test]
    fn test_pane_tracked_process_lifecycle_when_busy_output_moves_only_quiet_deadline() -> rootcause::Result<()> {
        let then = Instant::now();
        let visible_activity_at = self::instant_after(then, Duration::from_millis(501))?;
        let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, then);

        pretty_assertions::assert_eq!(
            pane_tracked_process.record_visible_activity(&self::tracked_process("codex")?, visible_activity_at),
            TrackedProcessChanges::deadline_only()
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process.quiet_deadline(TrackedProcessPaneFocus::Unfocused)?,
            Some(self::instant_after(visible_activity_at, Duration::from_secs(3))?)
        );
        Ok(())
    }

    #[test]
    fn test_pane_tracked_process_lifecycle_when_user_echo_suppression_expires_records_busy_activity()
    -> rootcause::Result<()> {
        let then = Instant::now();
        let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, then);
        pane_tracked_process.record_user_interaction(
            TrackedProcessUserInteraction::MayEcho,
            then,
            TrackedProcessPaneFocus::Focused,
        );
        let visible_activity_at = self::instant_after(then, Duration::from_millis(501))?;

        pretty_assertions::assert_eq!(
            pane_tracked_process.record_visible_activity(&self::tracked_process("codex")?, visible_activity_at),
            TrackedProcessChanges::deadline_only()
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process.quiet_deadline(TrackedProcessPaneFocus::Unfocused)?,
            Some(self::instant_after(visible_activity_at, Duration::from_secs(3))?)
        );
        Ok(())
    }

    #[test]
    fn test_pane_tracked_process_lifecycle_when_prompt_submit_precedes_visible_activity_marks_busy()
    -> rootcause::Result<()> {
        let then = Instant::now();
        let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, then);
        pane_tracked_process.status = PaneTrackedProcessStatus::Seen;
        pane_tracked_process.record_user_interaction(
            TrackedProcessUserInteraction::MayEcho,
            then,
            TrackedProcessPaneFocus::Focused,
        );
        pretty_assertions::assert_eq!(
            pane_tracked_process.record_user_interaction(
                TrackedProcessUserInteraction::StartsTrackedProcessWork,
                self::instant_after(then, Duration::from_millis(100))?,
                TrackedProcessPaneFocus::Focused,
            ),
            TrackedProcessChanges::state_and_deadline()
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process.record_visible_activity(
                &self::tracked_process("codex")?,
                self::instant_after(then, Duration::from_millis(150))?,
            ),
            TrackedProcessChanges::deadline_only()
        );

        pretty_assertions::assert_eq!(pane_tracked_process.state(), TrackedProcessState::Busy);
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
        let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, then);

        pretty_assertions::assert_eq!(
            pane_tracked_process.mark_quiet_if_due(self::instant_after(then, Duration::from_secs(3))?, focus_state),
            TrackedProcessStateChange::Changed
        );

        pretty_assertions::assert_eq!(pane_tracked_process.state(), expected_state);
        Ok(())
    }

    #[test]
    fn test_pane_tracked_process_lifecycle_when_focused_user_input_is_recent_stays_busy() -> rootcause::Result<()> {
        let then = Instant::now();
        let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, then);
        pretty_assertions::assert_eq!(
            pane_tracked_process.record_user_interaction(
                TrackedProcessUserInteraction::MayEcho,
                self::instant_after(then, Duration::from_secs(2))?,
                TrackedProcessPaneFocus::Focused,
            ),
            TrackedProcessChanges::deadline_only()
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process.mark_quiet_if_due(
                self::instant_after(then, Duration::from_secs(3))?,
                TrackedProcessPaneFocus::Focused
            ),
            TrackedProcessStateChange::Unchanged
        );
        pretty_assertions::assert_eq!(pane_tracked_process.state(), TrackedProcessState::Busy);

        pretty_assertions::assert_eq!(
            pane_tracked_process.mark_quiet_if_due(
                self::instant_after(then, Duration::from_secs(5))?,
                TrackedProcessPaneFocus::Focused
            ),
            TrackedProcessStateChange::Changed
        );
        pretty_assertions::assert_eq!(pane_tracked_process.state(), TrackedProcessState::Seen);
        Ok(())
    }

    #[test]
    fn test_pane_tracked_process_lifecycle_when_unfocused_user_input_is_recent_still_marks_unseen()
    -> rootcause::Result<()> {
        let then = Instant::now();
        let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, then);
        pretty_assertions::assert_eq!(
            pane_tracked_process.record_user_interaction(
                TrackedProcessUserInteraction::MayEcho,
                self::instant_after(then, Duration::from_secs(2))?,
                TrackedProcessPaneFocus::Unfocused,
            ),
            TrackedProcessChanges::default()
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process.mark_quiet_if_due(
                self::instant_after(then, Duration::from_secs(3))?,
                TrackedProcessPaneFocus::Unfocused
            ),
            TrackedProcessStateChange::Changed
        );

        pretty_assertions::assert_eq!(pane_tracked_process.state(), TrackedProcessState::Unseen);
        Ok(())
    }

    #[test]
    fn test_pane_tracked_process_lifecycle_when_attention_is_acknowledged_marks_seen() -> rootcause::Result<()> {
        let mut pane_tracked_process =
            PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, Instant::now());
        pane_tracked_process.status = PaneTrackedProcessStatus::Unseen;

        pretty_assertions::assert_eq!(
            pane_tracked_process.acknowledge_attention(),
            TrackedProcessStateChange::Changed
        );

        pretty_assertions::assert_eq!(pane_tracked_process.state(), TrackedProcessState::Seen);
        Ok(())
    }

    #[test]
    fn test_observe_pane_cmd_when_tracked_process_is_fg_marks_busy() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let pane_id = self::pane_id()?;

        assert2::assert!(
            pane_tracked_processes
                .observe_pane_cmd(
                    &MuxrConfig::default(),
                    pane_id,
                    &self::fg_tracked_process("codex"),
                    Instant::now(),
                )
                .state_change()
                == TrackedProcessStateChange::Changed
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Busy
        );
        let snapshot = pane_tracked_processes.snapshot(&layout);
        let pane = self::tracked_process_snapshot_pane(&snapshot, pane_id)?;
        pretty_assertions::assert_eq!(pane.label(), "cx");
        pretty_assertions::assert_eq!(pane.state(), TrackedProcessState::Busy);
        Ok(())
    }

    #[test]
    fn test_observe_pane_cmd_when_tracked_process_is_fg_group_member_marks_busy() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let pane_id = self::pane_id()?;

        assert2::assert!(
            pane_tracked_processes
                .observe_pane_cmd(
                    &MuxrConfig::default(),
                    pane_id,
                    &PaneCmdObservation::FgCmd(FgCmd::from_test_group(
                        Some(self::cmd(17869, "ags")),
                        Ok(vec![self::cmd(17989, "codex")]),
                    )),
                    Instant::now(),
                )
                .state_change()
                == TrackedProcessStateChange::Changed
        );

        let snapshot = pane_tracked_processes.snapshot(&layout);
        let pane = self::tracked_process_snapshot_pane(&snapshot, pane_id)?;
        pretty_assertions::assert_eq!(pane.label(), "cx");
        pretty_assertions::assert_eq!(pane.state(), TrackedProcessState::Busy);
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
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        self::set_pane_tracked_process_status(&mut pane_tracked_processes, pane_id, PaneTrackedProcessStatus::Unseen);

        assert2::assert!(
            pane_tracked_processes
                .observe_pane_cmd(
                    &MuxrConfig::default(),
                    pane_id,
                    &observation,
                    self::instant_after(then, Duration::from_secs(1))?,
                )
                .state_change()
                == TrackedProcessStateChange::Changed
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::None
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
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );

        pretty_assertions::assert_eq!(
            pane_tracked_processes.observe_visible_activity(
                &MuxrConfig::default(),
                pane_id,
                &observation,
                self::instant_after(then, Duration::from_secs(1))?,
            ),
            TrackedProcessChanges::state_and_deadline()
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::None
        );
        let snapshot = pane_tracked_processes.snapshot(&layout);
        assert2::assert!(self::tracked_process_snapshot_pane(&snapshot, pane_id).is_err());
        pretty_assertions::assert_eq!(pane_tracked_processes.next_quiet_deadline(&layout)?, None);
        Ok(())
    }

    #[test]
    fn test_observe_pane_cmd_when_unknown_preserves_state() -> rootcause::Result<()> {
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        self::set_pane_tracked_process_status(&mut pane_tracked_processes, pane_id, PaneTrackedProcessStatus::Unseen);

        assert2::assert!(
            pane_tracked_processes
                .observe_pane_cmd(
                    &MuxrConfig::default(),
                    pane_id,
                    &self::unknown(),
                    self::instant_after(then, Duration::from_secs(1))?,
                )
                .presence()
                == TrackedProcessChangePresence::Empty
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Unseen
        );
        Ok(())
    }

    #[test]
    fn test_observe_visible_activity_when_unseen_tracked_process_repaints_without_prompt_keeps_attention()
    -> rootcause::Result<()> {
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let pane_id = self::pane_id()?;
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            Instant::now(),
        );
        self::set_pane_tracked_process_status(&mut pane_tracked_processes, pane_id, PaneTrackedProcessStatus::Unseen);

        pretty_assertions::assert_eq!(
            pane_tracked_processes.observe_visible_activity(
                &MuxrConfig::default(),
                pane_id,
                &self::fg_tracked_process("codex"),
                Instant::now(),
            ),
            TrackedProcessChanges::default()
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Unseen
        );
        Ok(())
    }

    #[test]
    fn test_observe_visible_activity_when_cursor_repaints_after_seen_does_not_mark_busy() -> rootcause::Result<()> {
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let pane_id = self::pane_id()?;
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("cursor-agent"),
            Instant::now(),
        );
        self::set_pane_tracked_process_status(&mut pane_tracked_processes, pane_id, PaneTrackedProcessStatus::Seen);

        pretty_assertions::assert_eq!(
            pane_tracked_processes.observe_visible_activity(
                &MuxrConfig::default(),
                pane_id,
                &self::fg_tracked_process("cursor-agent"),
                Instant::now(),
            ),
            TrackedProcessChanges::default()
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Seen
        );
        Ok(())
    }

    #[test]
    fn test_observe_visible_activity_when_user_echoes_output_does_not_mark_busy() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        self::set_pane_tracked_process_status(&mut pane_tracked_processes, pane_id, PaneTrackedProcessStatus::Seen);
        pane_tracked_processes.record_user_interaction(
            &layout,
            pane_id,
            TrackedProcessUserInteraction::MayEcho,
            then,
        )?;

        pretty_assertions::assert_eq!(
            pane_tracked_processes.observe_visible_activity(
                &MuxrConfig::default(),
                pane_id,
                &self::fg_tracked_process("codex"),
                self::instant_after(then, Duration::from_millis(100))?,
            ),
            TrackedProcessChanges::default()
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Seen
        );
        Ok(())
    }

    #[test]
    fn test_observe_visible_activity_when_prompt_submit_precedes_output_marks_busy() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let pane_id = self::pane_id()?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        self::set_pane_tracked_process_status(&mut pane_tracked_processes, pane_id, PaneTrackedProcessStatus::Seen);
        pane_tracked_processes.record_user_interaction(
            &layout,
            pane_id,
            TrackedProcessUserInteraction::MayEcho,
            then,
        )?;
        pane_tracked_processes.record_user_interaction(
            &layout,
            pane_id,
            TrackedProcessUserInteraction::StartsTrackedProcessWork,
            self::instant_after(then, Duration::from_millis(100))?,
        )?;

        pretty_assertions::assert_eq!(
            pane_tracked_processes.observe_visible_activity(
                &MuxrConfig::default(),
                pane_id,
                &self::fg_tracked_process("codex"),
                self::instant_after(then, Duration::from_millis(150))?,
            ),
            TrackedProcessChanges::deadline_only()
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Busy
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
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        pane_tracked_processes.record_user_interaction(
            &layout,
            pane_id,
            TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_millis(100))?,
        )?;

        assert2::assert!(
            pane_tracked_processes
                .observe_pane_cmd(
                    &MuxrConfig::default(),
                    pane_id,
                    &self::fg_tracked_process("cursor-agent"),
                    self::instant_after(then, Duration::from_millis(150))?,
                )
                .state_change()
                == TrackedProcessStateChange::Changed
        );
        self::set_pane_tracked_process_status(&mut pane_tracked_processes, pane_id, PaneTrackedProcessStatus::Seen);
        pane_tracked_processes.record_user_interaction(
            &layout,
            pane_id,
            TrackedProcessUserInteraction::StartsTrackedProcessWork,
            self::instant_after(then, Duration::from_millis(175))?,
        )?;
        pretty_assertions::assert_eq!(
            pane_tracked_processes.observe_visible_activity(
                &MuxrConfig::default(),
                pane_id,
                &self::fg_tracked_process("cursor-agent"),
                self::instant_after(then, Duration::from_millis(200))?,
            ),
            TrackedProcessChanges::deadline_only()
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Busy
        );
        let snapshot = pane_tracked_processes.snapshot(&layout);
        let pane = self::tracked_process_snapshot_pane(&snapshot, pane_id)?;
        pretty_assertions::assert_eq!(pane.label(), "cu");
        pretty_assertions::assert_eq!(pane.state(), TrackedProcessState::Busy);
        Ok(())
    }

    #[test]
    fn test_mark_quiet_deadlines_when_unfocused_busy_tracked_process_is_quiet_marks_unseen() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let pane_id = PaneId::new(1)?;
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );

        pretty_assertions::assert_eq!(
            pane_tracked_processes.next_quiet_deadline(&layout)?,
            Some(self::instant_after(then, Duration::from_secs(3))?)
        );
        let outcome =
            pane_tracked_processes.mark_quiet_deadlines(&layout, self::instant_after(then, Duration::from_secs(3))?)?;
        pretty_assertions::assert_eq!(
            outcome,
            TrackedProcessAttention::Unseen {
                pane_ids: vec![pane_id]
            }
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Unseen
        );
        pretty_assertions::assert_eq!(pane_tracked_processes.attention_pane_ids(&layout), vec![pane_id]);
        pretty_assertions::assert_eq!(pane_tracked_processes.next_quiet_deadline(&layout)?, None);
        Ok(())
    }

    #[test]
    fn test_next_quiet_deadline_when_focused_user_input_is_recent_uses_user_input() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new(1)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
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

        pretty_assertions::assert_eq!(
            pane_tracked_processes.next_quiet_deadline(&layout)?,
            Some(self::instant_after(then, Duration::from_secs(5))?)
        );
        Ok(())
    }

    #[test]
    fn test_next_quiet_deadline_when_unfocused_user_input_precedes_focus_uses_visible_activity() -> rootcause::Result<()>
    {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new(1)?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
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

        layout.active_tab_mut()?.focus_pane(pane_id)?;

        pretty_assertions::assert_eq!(
            pane_tracked_processes.next_quiet_deadline(&layout)?,
            Some(self::instant_after(then, Duration::from_secs(3))?)
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
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );

        let outcome =
            pane_tracked_processes.mark_quiet_deadlines(&layout, self::instant_after(then, Duration::from_secs(3))?)?;

        pretty_assertions::assert_eq!(outcome, TrackedProcessAttention::Seen);
        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Seen
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
            &MuxrConfig::default(),
            reused_pane_id,
            &self::fg_tracked_process("codex"),
            Instant::now(),
        );

        assert2::assert!(
            pane_tracked_processes.remove_pane(reused_pane_id).state_change() == TrackedProcessStateChange::Changed
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
            MuxrConfig::default().layout,
            self::metadata("sh", 3),
            PaneSplitAxis::Vertical,
        )?;

        pretty_assertions::assert_eq!(new_pane_id, reused_pane_id);
        let snapshot = pane_tracked_processes.snapshot(&layout);
        assert2::assert!(self::tracked_process_snapshot_pane(&snapshot, reused_pane_id).is_err());
        pretty_assertions::assert_eq!(pane_tracked_processes.next_quiet_deadline(&layout)?, None);
        Ok(())
    }

    #[test]
    fn test_next_quiet_deadline_when_state_references_removed_pane_ignores_stale_pane() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let stale_pane_id = PaneId::new(2)?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            stale_pane_id,
            &self::fg_tracked_process("codex"),
            Instant::now(),
        );

        layout.remove_exited_pane(stale_pane_id, 0, self::successful_exit_status())?;

        pretty_assertions::assert_eq!(pane_tracked_processes.next_quiet_deadline(&layout)?, None);
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
            &MuxrConfig::default(),
            stale_pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            live_pane_id,
            &self::fg_tracked_process("codex"),
            self::instant_after(then, Duration::from_secs(1))?,
        );
        layout.remove_exited_pane(stale_pane_id, 0, self::successful_exit_status())?;

        pretty_assertions::assert_eq!(
            pane_tracked_processes.next_quiet_deadline(&layout)?,
            Some(self::instant_after(then, Duration::from_secs(4))?)
        );
        Ok(())
    }

    #[test]
    fn test_acknowledge_attention_when_tracked_process_is_unseen_marks_seen() -> rootcause::Result<()> {
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let pane_id = self::pane_id()?;
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            Instant::now(),
        );
        self::set_pane_tracked_process_status(&mut pane_tracked_processes, pane_id, PaneTrackedProcessStatus::Unseen);

        assert2::assert!(
            pane_tracked_processes.acknowledge_attention(pane_id).state_change() == TrackedProcessStateChange::Changed
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Seen
        );
        Ok(())
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
            MuxrConfig::default().layout,
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

    fn instant_after(instant: Instant, duration: Duration) -> rootcause::Result<Instant> {
        instant
            .checked_add(duration)
            .ok_or_else(|| rootcause::report!("test instant overflowed"))
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
}
