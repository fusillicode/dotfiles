use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use muxr_config::MuxrConfig;
use muxr_config::TrackedProcess;
use muxr_core::PaneId;
use muxr_core::TrackedProcessState;

use crate::pane_cmd::PaneCmdObservation;
use crate::pane_cmd::PaneCmdSnapshot;
use crate::pane_runtime::PaneRuntimes;
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

#[derive(Debug)]
struct PaneTrackedProcessLifecycle {
    last_visible_activity: Instant,
    pending_work_start_at: Option<Instant>,
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
            last_visible_activity: now,
            pending_work_start_at: None,
            recent_user_interaction: None,
            status: PaneTrackedProcessStatus::Busy,
            tracked_process,
        }
    }

    fn observe_tracked_process(&mut self, tracked_process: &TrackedProcess, now: Instant) -> bool {
        if self.tracked_process.id == tracked_process.id {
            return false;
        }

        // A different tracked foreground process starts a new lifecycle; old activity, attention, and local-echo
        // suppression belonged to the previous process and must not carry over.
        *self = Self::new(tracked_process.clone(), now);
        true
    }

    const fn record_user_interaction(&mut self, interaction: TrackedProcessUserInteraction, now: Instant) {
        match interaction {
            TrackedProcessUserInteraction::MayEcho => {
                self.recent_user_interaction = Some(now);
            }
            TrackedProcessUserInteraction::StartsTrackedProcessWork => {
                // Submitting a prompt is user input, but the next redraw is tracked-process work. Clear prior typing
                // suppression so a fast response is not lost as local echo.
                self.pending_work_start_at = Some(now);
                self.recent_user_interaction = None;
            }
        }
    }

    fn record_visible_activity(&mut self, tracked_process: &TrackedProcess, now: Instant) -> bool {
        if self.tracked_process.id != tracked_process.id {
            return false;
        }
        self.discard_stale_user_interaction(now);
        if self.recent_user_interaction.is_some() {
            // User typing and mouse gestures can redraw through the PTY. Those bytes still render, but they are not
            // tracked-process work and must not flip attention back to Busy. Keep suppression for the short window;
            // prompt submit clears it explicitly with `StartsTrackedProcessWork`.
            return false;
        }
        if self.status != PaneTrackedProcessStatus::Busy && self.pending_work_start_at.is_none() {
            // Some terminal apps can repaint idle UI while unfocused. After startup/work has been acknowledged, only
            // a prompt submit is allowed to re-arm tracked-process attention from visible output.
            return false;
        }

        self.pending_work_start_at = None;
        self.last_visible_activity = now;
        self.mark_visible_activity()
    }

    fn mark_quiet_if_due(&mut self, now: Instant, focused: bool) -> bool {
        self.mark_quiet(now.saturating_duration_since(self.last_visible_activity), focused)
    }

    const fn mark_visible_activity(&mut self) -> bool {
        match self.status {
            PaneTrackedProcessStatus::Busy => false,
            PaneTrackedProcessStatus::Seen | PaneTrackedProcessStatus::Unseen => {
                self.status = PaneTrackedProcessStatus::Busy;
                true
            }
        }
    }

    fn mark_quiet(&mut self, quiet_for: Duration, focused: bool) -> bool {
        if self.status != PaneTrackedProcessStatus::Busy {
            return false;
        }
        if quiet_for < self.tracked_process.quiet_threshold {
            return false;
        }

        self.status = if focused {
            PaneTrackedProcessStatus::Seen
        } else {
            PaneTrackedProcessStatus::Unseen
        };
        true
    }

    const fn needs_attention(&self) -> bool {
        matches!(self.status, PaneTrackedProcessStatus::Unseen)
    }

    const fn state(&self) -> TrackedProcessState {
        match self.status {
            PaneTrackedProcessStatus::Busy => TrackedProcessState::Busy,
            PaneTrackedProcessStatus::Seen => TrackedProcessState::Seen,
            PaneTrackedProcessStatus::Unseen => TrackedProcessState::Unseen,
        }
    }

    const fn acknowledge_attention(&mut self) -> bool {
        if !self.needs_attention() {
            return false;
        }
        self.status = PaneTrackedProcessStatus::Seen;
        true
    }

    fn discard_stale_user_interaction(&mut self, now: Instant) {
        let Some(last_activity) = self.recent_user_interaction else {
            return;
        };
        if now.saturating_duration_since(last_activity) > USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION {
            self.recent_user_interaction = None;
        }
    }

    fn quiet_deadline(&self) -> rootcause::Result<Option<Instant>> {
        if self.status != PaneTrackedProcessStatus::Busy {
            return Ok(None);
        }
        self.last_visible_activity
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
    ) -> rootcause::Result<bool> {
        let pane_ids = layout.panes().into_iter().map(|pane| pane.id).collect::<Vec<_>>();
        self.observe_runtime_pane_cmds(config, runtimes, &pane_ids, now)
    }

    pub fn observe_runtime_pane_cmds(
        &mut self,
        config: &MuxrConfig,
        runtimes: &PaneRuntimes,
        pane_ids: &[PaneId],
        now: Instant,
    ) -> rootcause::Result<bool> {
        let mut changed = false;
        for pane_id in pane_ids {
            let observation = self::runtime_pane_cmd_observation(runtimes, *pane_id)?;
            changed |= self.observe_pane_cmd(config, *pane_id, &observation, now);
        }
        Ok(changed)
    }

    pub fn observe_runtime_visible_activity(
        &mut self,
        config: &MuxrConfig,
        runtimes: &PaneRuntimes,
        pane_ids: &[PaneId],
        now: Instant,
    ) -> rootcause::Result<bool> {
        let mut changed = false;
        for pane_id in pane_ids {
            let observation = self::runtime_pane_cmd_observation(runtimes, *pane_id)?;
            changed |= self.observe_visible_activity(config, *pane_id, &observation, now);
        }
        Ok(changed)
    }

    pub fn acknowledge_active_pane_attention(
        &mut self,
        config: &MuxrConfig,
        layout: &SessionLayout,
        runtimes: &PaneRuntimes,
        now: Instant,
    ) -> rootcause::Result<bool> {
        let active_pane = layout.active_pane_id()?;
        let observation = self::runtime_pane_cmd_observation(runtimes, active_pane)?;
        let mut changed = self.observe_pane_cmd(config, active_pane, &observation, now);
        changed |= self.acknowledge_attention(active_pane);
        Ok(changed)
    }

    pub fn observe_pane_cmd(
        &mut self,
        config: &MuxrConfig,
        pane_id: PaneId,
        observation: &PaneCmdObservation,
        now: Instant,
    ) -> bool {
        self.apply_cmd_observation(
            pane_id,
            self::tracked_process_observation_from_pane_cmd(config, observation),
            now,
        )
    }

    pub fn observe_visible_activity(
        &mut self,
        config: &MuxrConfig,
        pane_id: PaneId,
        observation: &PaneCmdObservation,
        now: Instant,
    ) -> bool {
        let cmd_observation = self::tracked_process_observation_from_pane_cmd(config, observation);
        let tracked_process = match cmd_observation {
            TrackedProcessCmdObservation::Tracked(tracked_process) => Some(tracked_process),
            TrackedProcessCmdObservation::TrustedUntracked | TrackedProcessCmdObservation::Unknown => None,
        };
        let mut changed = self.apply_cmd_observation(pane_id, cmd_observation, now);
        changed |= self.record_visible_activity(pane_id, tracked_process, now);
        changed
    }

    pub fn mark_quiet_deadlines(
        &mut self,
        layout: &SessionLayout,
        now: Instant,
    ) -> rootcause::Result<TrackedProcessAttention> {
        self.retain_layout_panes(layout);
        self.discard_stale_user_interactions(now);
        self.mark_quiet_tracked_processes(layout, now)
    }

    pub fn attention_pane_ids(&self, layout: &SessionLayout) -> Vec<PaneId> {
        layout
            .panes()
            .into_iter()
            .filter(|pane| self.needs_attention(pane.id))
            .map(|pane| pane.id)
            .collect()
    }

    pub fn next_quiet_deadline(&self) -> rootcause::Result<Option<Instant>> {
        let mut deadline = None;
        for pane_tracked_process in self.by_pane.values() {
            let Some(pane_deadline) = pane_tracked_process.quiet_deadline()? else {
                continue;
            };
            deadline = Some(deadline.map_or(pane_deadline, |current: Instant| current.min(pane_deadline)));
        }
        Ok(deadline)
    }

    pub fn snapshot(&self) -> PaneTrackedProcessSnapshot {
        let panes = self
            .by_pane
            .iter()
            .map(|(pane_id, pane_tracked_process)| {
                (
                    *pane_id,
                    PaneTrackedProcessSnapshotEntry {
                        label: pane_tracked_process.tracked_process.label.to_owned(),
                        state: pane_tracked_process.state(),
                    },
                )
            })
            .collect();
        PaneTrackedProcessSnapshot { panes }
    }

    fn mark_quiet_tracked_processes(
        &mut self,
        layout: &SessionLayout,
        now: Instant,
    ) -> rootcause::Result<TrackedProcessAttention> {
        let focused_pane = layout.active_pane_id()?;
        let pane_ids = self::layout_pane_ids(layout);
        let mut seen = false;
        let mut unseen_panes = Vec::new();
        for pane_id in pane_ids {
            let focused = pane_id == focused_pane;
            if self.mark_quiet_if_due(pane_id, now, focused) {
                // The state machine owns first-time attention transitions; callers should react to this outcome instead
                // of diffing snapshots and duplicating status rules outside this feature.
                if focused {
                    seen = true;
                } else if self.needs_attention(pane_id) {
                    unseen_panes.push(pane_id);
                }
            }
        }
        if !unseen_panes.is_empty() {
            Ok(TrackedProcessAttention::Unseen { pane_ids: unseen_panes })
        } else if seen {
            Ok(TrackedProcessAttention::Seen)
        } else {
            Ok(TrackedProcessAttention::Unchanged)
        }
    }

    fn retain_layout_panes(&mut self, layout: &SessionLayout) {
        let pane_ids = self::layout_pane_ids(layout).into_iter().collect();
        self.retain_panes(&pane_ids);
    }

    fn apply_cmd_observation(
        &mut self,
        pane_id: PaneId,
        observation: TrackedProcessCmdObservation<'_>,
        now: Instant,
    ) -> bool {
        match observation {
            TrackedProcessCmdObservation::Tracked(tracked_process) => {
                self.observe_tracked_process(pane_id, tracked_process, now)
            }
            TrackedProcessCmdObservation::TrustedUntracked => {
                // A trusted shell/untracked observation ends the current lifecycle. Unknown observations do not.
                self.by_pane.remove(&pane_id).is_some()
            }
            TrackedProcessCmdObservation::Unknown => false,
        }
    }

    fn observe_tracked_process(&mut self, pane_id: PaneId, tracked_process: &TrackedProcess, now: Instant) -> bool {
        let Some(lifecycle) = self.by_pane.get_mut(&pane_id) else {
            // A newly observed tracked process starts Busy even if its first dirty frame was missed by the sampled
            // scan. Pre-process input suppression belongs to the shell and must not hide the new process output.
            self.by_pane
                .insert(pane_id, PaneTrackedProcessLifecycle::new(tracked_process.clone(), now));
            return true;
        };
        lifecycle.observe_tracked_process(tracked_process, now)
    }

    pub fn record_user_interaction(
        &mut self,
        pane_id: PaneId,
        interaction: TrackedProcessUserInteraction,
        now: Instant,
    ) {
        if let Some(lifecycle) = self.by_pane.get_mut(&pane_id) {
            lifecycle.record_user_interaction(interaction, now);
        }
    }

    fn record_visible_activity(
        &mut self,
        pane_id: PaneId,
        tracked_process: Option<&TrackedProcess>,
        now: Instant,
    ) -> bool {
        let Some(tracked_process) = tracked_process else {
            // PTY output before process detection is still rendered, but it is not tracked-process activity yet.
            // Newly detected tracked processes start Busy instead of inheriting stale shell output.
            return false;
        };
        let Some(pane_tracked_process) = self.by_pane.get_mut(&pane_id) else {
            return false;
        };
        pane_tracked_process.record_visible_activity(tracked_process, now)
    }

    fn mark_quiet_if_due(&mut self, pane_id: PaneId, now: Instant, focused: bool) -> bool {
        let Some(pane_tracked_process) = self.by_pane.get_mut(&pane_id) else {
            return false;
        };
        pane_tracked_process.mark_quiet_if_due(now, focused)
    }

    pub fn acknowledge_attention(&mut self, pane_id: PaneId) -> bool {
        self.by_pane
            .get_mut(&pane_id)
            .is_some_and(PaneTrackedProcessLifecycle::acknowledge_attention)
    }

    fn needs_attention(&self, pane_id: PaneId) -> bool {
        self.by_pane
            .get(&pane_id)
            .is_some_and(PaneTrackedProcessLifecycle::needs_attention)
    }

    fn retain_panes(&mut self, pane_ids: &BTreeSet<PaneId>) {
        self.by_pane
            .retain(|pane_id, _pane_tracked_process| pane_ids.contains(pane_id));
    }

    fn discard_stale_user_interactions(&mut self, now: Instant) {
        for pane_tracked_process in self.by_pane.values_mut() {
            pane_tracked_process.discard_stale_user_interaction(now);
        }
    }
}

fn layout_pane_ids(layout: &SessionLayout) -> Vec<PaneId> {
    layout.panes().into_iter().map(|pane| pane.id).collect()
}

fn tracked_process_observation_from_pane_cmd<'a>(
    config: &'a MuxrConfig,
    observation: &PaneCmdObservation,
) -> TrackedProcessCmdObservation<'a> {
    match observation {
        PaneCmdObservation::FgCmd { .. } => self::tracked_process_from_pane_cmd(config, observation).map_or(
            TrackedProcessCmdObservation::TrustedUntracked,
            TrackedProcessCmdObservation::Tracked,
        ),
        PaneCmdObservation::Shell => TrackedProcessCmdObservation::TrustedUntracked,
        PaneCmdObservation::Unknown { .. } => TrackedProcessCmdObservation::Unknown,
    }
}

fn tracked_process_from_pane_cmd<'a>(
    config: &'a MuxrConfig,
    observation: &PaneCmdObservation,
) -> Option<&'a TrackedProcess> {
    let PaneCmdObservation::FgCmd { cmd } = observation else {
        return None;
    };
    config.tracked_process_for_cmd(&cmd.executable, cmd.path.as_deref())
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
    use crate::pane_cmd::PaneCmd;
    use crate::pane_cmd::PaneCmdUnknownReason;
    use crate::pane_split::PaneSplitAxis;
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
        pane_tracked_process.record_user_interaction(TrackedProcessUserInteraction::MayEcho, then);

        assert2::assert!(!pane_tracked_process.record_visible_activity(
            &self::tracked_process("codex")?,
            self::instant_after(then, Duration::from_millis(100))?
        ));

        pretty_assertions::assert_eq!(pane_tracked_process.state(), expected_state);
        Ok(())
    }

    #[test]
    fn test_pane_tracked_process_lifecycle_when_user_echo_suppression_expires_records_busy_activity()
    -> rootcause::Result<()> {
        let then = Instant::now();
        let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, then);
        pane_tracked_process.record_user_interaction(TrackedProcessUserInteraction::MayEcho, then);
        let visible_activity_at = self::instant_after(then, Duration::from_millis(501))?;

        assert2::assert!(
            !pane_tracked_process.record_visible_activity(&self::tracked_process("codex")?, visible_activity_at)
        );

        pretty_assertions::assert_eq!(
            pane_tracked_process.quiet_deadline()?,
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
        pane_tracked_process.record_user_interaction(TrackedProcessUserInteraction::MayEcho, then);
        pane_tracked_process.record_user_interaction(
            TrackedProcessUserInteraction::StartsTrackedProcessWork,
            self::instant_after(then, Duration::from_millis(100))?,
        );

        assert2::assert!(pane_tracked_process.record_visible_activity(
            &self::tracked_process("codex")?,
            self::instant_after(then, Duration::from_millis(150))?
        ));

        pretty_assertions::assert_eq!(pane_tracked_process.state(), TrackedProcessState::Busy);
        Ok(())
    }

    #[rstest::rstest]
    #[case::focused(true, TrackedProcessState::Seen)]
    #[case::unfocused(false, TrackedProcessState::Unseen)]
    fn test_pane_tracked_process_lifecycle_when_quiet_deadline_fires_marks_seen_or_unseen(
        #[case] focused: bool,
        #[case] expected_state: TrackedProcessState,
    ) -> rootcause::Result<()> {
        let then = Instant::now();
        let mut pane_tracked_process = PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, then);

        assert2::assert!(
            pane_tracked_process.mark_quiet_if_due(self::instant_after(then, Duration::from_secs(3))?, focused)
        );

        pretty_assertions::assert_eq!(pane_tracked_process.state(), expected_state);
        Ok(())
    }

    #[test]
    fn test_pane_tracked_process_lifecycle_when_attention_is_acknowledged_marks_seen() -> rootcause::Result<()> {
        let mut pane_tracked_process =
            PaneTrackedProcessLifecycle::new(self::tracked_process("codex")?, Instant::now());
        pane_tracked_process.status = PaneTrackedProcessStatus::Unseen;

        assert2::assert!(pane_tracked_process.acknowledge_attention());

        pretty_assertions::assert_eq!(pane_tracked_process.state(), TrackedProcessState::Seen);
        Ok(())
    }

    #[test]
    fn test_observe_pane_cmd_when_tracked_process_is_fg_marks_busy() -> rootcause::Result<()> {
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let pane_id = self::pane_id()?;

        assert2::assert!(pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            Instant::now(),
        ));

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Busy
        );
        let snapshot = pane_tracked_processes.snapshot();
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

        assert2::assert!(pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &observation,
            self::instant_after(then, Duration::from_secs(1))?,
        ));

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::None
        );
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

        assert2::assert!(!pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::unknown(),
            self::instant_after(then, Duration::from_secs(1))?,
        ));

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

        assert2::assert!(!pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            Instant::now(),
        ));

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

        assert2::assert!(!pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("cursor-agent"),
            Instant::now(),
        ));

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Seen
        );
        Ok(())
    }

    #[test]
    fn test_observe_visible_activity_when_user_echoes_output_does_not_mark_busy() -> rootcause::Result<()> {
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
        pane_tracked_processes.record_user_interaction(pane_id, TrackedProcessUserInteraction::MayEcho, then);

        assert2::assert!(!pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            self::instant_after(then, Duration::from_millis(100))?,
        ));

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Seen
        );
        Ok(())
    }

    #[test]
    fn test_observe_visible_activity_when_prompt_submit_precedes_output_marks_busy() -> rootcause::Result<()> {
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
        pane_tracked_processes.record_user_interaction(pane_id, TrackedProcessUserInteraction::MayEcho, then);
        pane_tracked_processes.record_user_interaction(
            pane_id,
            TrackedProcessUserInteraction::StartsTrackedProcessWork,
            self::instant_after(then, Duration::from_millis(100))?,
        );

        assert2::assert!(pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            self::instant_after(then, Duration::from_millis(150))?,
        ));

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Busy
        );
        Ok(())
    }

    #[test]
    fn test_observe_pane_cmd_when_tracked_process_identity_changes_resets_state() -> rootcause::Result<()> {
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        pane_tracked_processes.record_user_interaction(
            pane_id,
            TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_millis(100))?,
        );

        assert2::assert!(pane_tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("cursor-agent"),
            self::instant_after(then, Duration::from_millis(150))?,
        ));
        self::set_pane_tracked_process_status(&mut pane_tracked_processes, pane_id, PaneTrackedProcessStatus::Seen);
        pane_tracked_processes.record_user_interaction(
            pane_id,
            TrackedProcessUserInteraction::StartsTrackedProcessWork,
            self::instant_after(then, Duration::from_millis(175))?,
        );
        assert2::assert!(pane_tracked_processes.observe_visible_activity(
            &MuxrConfig::default(),
            pane_id,
            &self::fg_tracked_process("cursor-agent"),
            self::instant_after(then, Duration::from_millis(200))?,
        ));

        pretty_assertions::assert_eq!(
            pane_tracked_process_status(&pane_tracked_processes, pane_id),
            TrackedProcessState::Busy
        );
        let snapshot = pane_tracked_processes.snapshot();
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
            pane_tracked_processes.next_quiet_deadline()?,
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
        pretty_assertions::assert_eq!(pane_tracked_processes.next_quiet_deadline()?, None);
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

        assert2::assert!(pane_tracked_processes.acknowledge_attention(pane_id));

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
        PaneCmdObservation::FgCmd {
            cmd: PaneCmd {
                executable: executable.to_owned(),
                path: None,
                pid: 42,
            },
        }
    }

    fn shell() -> PaneCmdObservation {
        PaneCmdObservation::Shell
    }

    fn unknown() -> PaneCmdObservation {
        PaneCmdObservation::Unknown {
            reason: PaneCmdUnknownReason::MissingFgProcess,
        }
    }
}
