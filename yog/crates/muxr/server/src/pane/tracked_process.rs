use std::collections::BTreeMap;
use std::collections::HashMap;
use std::time::Instant;

use muxr_config::MuxrConfig;
use muxr_config::TrackedProcess;
use muxr_core::PaneId;
use muxr_core::TrackedProcessState;

use self::lifecycle::PaneTrackedProcessLifecycle;
use self::lifecycle::TrackedProcessAttentionNeed;
use self::lifecycle::TrackedProcessPaneFocus;
use self::screen::ScreenObservation;
use crate::pane::cmd::FgCmd;
use crate::pane::cmd::PaneCmdObservation;
use crate::pane::cmd::PaneCmdSnapshot;
use crate::pane::cmd::ProcessGroupLookupError;
use crate::pane::runtime::PaneRuntimes;
use crate::pty::PtyHandle;
use crate::state::SessionLayout;
use crate::terminal::TerminalTextTail;

mod lifecycle;
mod screen;

#[cfg(test)]
mod tests;

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

impl TrackedProcessChanges {
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

    #[cfg(test)]
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

// Client-origin tracked-process changes must carry the pane id needed for sidebar/layout updates; keep them paired so
// callers cannot sync the timer and accidentally skip a visible state update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackedProcessClientChange {
    changes: TrackedProcessChanges,
    pane_id: PaneId,
}

impl TrackedProcessClientChange {
    pub const fn changes(self) -> TrackedProcessChanges {
        self.changes
    }

    pub const fn pane_id(self) -> PaneId {
        self.pane_id
    }

    fn from_changes(pane_id: PaneId, changes: TrackedProcessChanges) -> Option<Self> {
        (changes.presence() == TrackedProcessChangePresence::Present).then_some(Self { changes, pane_id })
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackedProcessUserInteraction {
    MayEcho,
    StartsTrackedProcessWork,
}

#[derive(Debug, Default)]
pub struct PaneTrackedProcesses {
    by_pane: HashMap<PaneId, PaneTrackedProcessLifecycle>,
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
            changes.merge(self.observe_runtime_pane_cmd(config, runtimes, *pane_id, now)?);
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
        let mut changes = self.observe_runtime_pane_cmd(config, runtimes, active_pane, now)?;
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

    #[cfg(test)]
    pub fn observe_visible_activity(
        &mut self,
        config: &MuxrConfig,
        pane_id: PaneId,
        observation: &PaneCmdObservation,
        now: Instant,
    ) -> TrackedProcessChanges {
        let cmd_observation = self::tracked_process_observation_from_pane_cmd(config, observation);
        let tracked = matches!(cmd_observation, TrackedProcessCmdObservation::Tracked(_));
        let state_change = self.apply_cmd_observation(pane_id, cmd_observation, now);
        let activity_changes = if tracked {
            self.by_pane
                .get_mut(&pane_id)
                .map_or_else(TrackedProcessChanges::default, |lifecycle| {
                    lifecycle.record_visible_activity(now)
                })
        } else {
            TrackedProcessChanges::default()
        };
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
                    label: pane_tracked_process.label().to_owned(),
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

    pub(crate) fn record_cached_visible_activity(
        &mut self,
        runtimes: &PaneRuntimes,
        pane_ids: &[PaneId],
        now: Instant,
    ) -> rootcause::Result<TrackedProcessChanges> {
        let mut changes = TrackedProcessChanges::default();
        for pane_id in pane_ids {
            let Some(lifecycle) = self.by_pane.get_mut(pane_id) else {
                continue;
            };
            let activity = match lifecycle.screen_observation() {
                Some(patterns) => {
                    let text = runtimes.handle(*pane_id)?.live_tail_text(screen::SCREEN_TAIL_ROWS);
                    lifecycle.record_screen_activity(screen::observe(patterns, text.candidate_line_spans()), now)
                }
                None => lifecycle.record_visible_activity(now),
            };
            changes.merge(activity);
        }
        Ok(changes)
    }

    /// Check the live screen immediately before quiet transitions. For confirmed agents with screen checks, require
    /// completion before red-dot activation, or cancellation for immediate clearing; otherwise the timer retries later.
    pub(crate) fn guard_quiet_deadlines(
        &mut self,
        config: &MuxrConfig,
        layout: &SessionLayout,
        runtimes: &PaneRuntimes,
        now: Instant,
    ) -> rootcause::Result<TrackedProcessChanges> {
        self.guard_observed_quiet_deadlines(
            layout,
            now,
            |pane_id| {
                let observation = self::runtime_pane_cmd_observation(runtimes, pane_id)?;
                Ok(self::tracked_process_observation_from_pane_cmd(config, &observation))
            },
            |pane_id| Ok(runtimes.handle(pane_id)?.live_tail_text(screen::SCREEN_TAIL_ROWS)),
        )
    }

    pub(in crate::pane) fn capture_completion_before_input(
        &mut self,
        pane_id: PaneId,
        interaction: TrackedProcessUserInteraction,
        handle: &PtyHandle,
    ) {
        if interaction != TrackedProcessUserInteraction::StartsTrackedProcessWork {
            return;
        }
        let Some(lifecycle) = self.by_pane.get_mut(&pane_id) else {
            return;
        };
        let Some(patterns) = lifecycle.completion_capture_patterns() else {
            return;
        };
        // Capture before writing input: the PTY reader can observe the next turn immediately after submission.
        let text = handle.live_tail_text(screen::SCREEN_TAIL_ROWS);
        let observation = screen::observe(patterns, text.candidate_line_spans());
        lifecycle.capture_completion_before_work(observation);
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
            // Pre-process input suppression belongs to the shell and must not hide the new process output.
            self.by_pane
                .insert(pane_id, PaneTrackedProcessLifecycle::new(tracked_process.clone(), now));
            return TrackedProcessStateChange::Changed;
        };
        lifecycle.observe_tracked_process(tracked_process, now)
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

    fn observe_runtime_pane_cmd(
        &mut self,
        config: &MuxrConfig,
        runtimes: &PaneRuntimes,
        pane_id: PaneId,
        now: Instant,
    ) -> rootcause::Result<TrackedProcessChanges> {
        let observation = self::runtime_pane_cmd_observation(runtimes, pane_id)?;
        let mut changes = self.observe_pane_cmd(config, pane_id, &observation, now);
        if changes.state_change() == TrackedProcessStateChange::Changed {
            // Discovery can follow the last dirty frame, including on attach or focus. Seed from the current screen.
            changes.merge(self.record_cached_visible_activity(runtimes, &[pane_id], now)?);
        }
        Ok(changes)
    }

    fn guard_observed_quiet_deadlines<'a>(
        &mut self,
        layout: &SessionLayout,
        now: Instant,
        mut observe_cmd: impl FnMut(PaneId) -> rootcause::Result<TrackedProcessCmdObservation<'a>>,
        mut read_screen: impl FnMut(PaneId) -> rootcause::Result<TerminalTextTail>,
    ) -> rootcause::Result<TrackedProcessChanges> {
        let focused_pane = layout.active_pane_id()?;
        let mut changes = TrackedProcessChanges::default();

        for pane_id in layout.pane_ids() {
            let focus = if pane_id == focused_pane {
                TrackedProcessPaneFocus::Focused
            } else {
                TrackedProcessPaneFocus::Unfocused
            };

            let Some(lifecycle) = self.by_pane.get(&pane_id) else {
                continue;
            };
            if lifecycle.screen_observation().is_none() {
                continue;
            }
            if lifecycle.quiet_deadline(focus)?.is_none_or(|deadline| deadline > now) {
                continue;
            }

            // An exited agent can leave its last status row behind. Refresh its identity before trusting that row.
            let observation = observe_cmd(pane_id)?;

            if self.apply_cmd_observation(pane_id, observation, now) == TrackedProcessStateChange::Changed {
                changes.merge(TrackedProcessChanges::state_and_deadline());
                continue;
            }
            let Some(lifecycle) = self.by_pane.get_mut(&pane_id) else {
                continue;
            };

            let guarded = if matches!(observation, TrackedProcessCmdObservation::Tracked(_)) {
                let text = read_screen(pane_id)?;
                let screen = lifecycle
                    .screen_observation()
                    .map_or(ScreenObservation::Unknown, |patterns| {
                        screen::observe(patterns, text.candidate_line_spans())
                    });
                lifecycle.guard_quiet_deadline(screen, now)
            } else {
                // Unknown identity retains the lifecycle and defers its quiet transition.
                lifecycle.guard_quiet_deadline(ScreenObservation::Unknown, now)
            };
            changes.merge(guarded);
        }

        Ok(changes)
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TrackedProcessChange {
    #[default]
    None,
    Deadline,
    State,
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

// Observations borrow the read-only config entry so hot visible-activity samples do not clone matcher Vecs. The
// lifecycle clones only when it actually stores a newly tracked process.
#[derive(Clone, Copy, Debug)]
enum TrackedProcessCmdObservation<'a> {
    Tracked(&'a TrackedProcess),
    TrustedUntracked,
    Unknown,
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
