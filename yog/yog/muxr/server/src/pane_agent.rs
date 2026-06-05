use std::collections::BTreeSet;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use muxr_core::PaneAgentState;
use muxr_core::PaneId;
use sysinfo::Pid;
use sysinfo::Process;
use sysinfo::ProcessRefreshKind;
use sysinfo::ProcessesToUpdate;
use sysinfo::System;
use sysinfo::UpdateKind;
use ytil_agents::agent::Agent;

use crate::state::SessionLayout;

const USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION: Duration = Duration::from_millis(500);
pub const AGENT_ATTENTION_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug)]
struct PaneAgentDetector {
    system: System,
}

#[derive(Debug, Default)]
pub struct PaneAgents {
    by_pane: HashMap<PaneId, PaneAgent>,
    recent_user_interaction_by_pane: HashMap<PaneId, Instant>,
}

pub struct PaneAgentDetectionWorker {
    next_request_id: u64,
    pending_request_id: Option<u64>,
    request_sender: mpsc::Sender<PaneAgentDetectionRequest>,
    response_receiver: mpsc::Receiver<PaneAgentDetectionResponse>,
}

#[derive(Debug)]
struct PaneAgent {
    agent: Agent,
    last_visible_activity: Instant,
    status: PaneAgentStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneAgentStatus {
    Busy,
    Seen,
    Unseen,
}

impl From<PaneAgentStatus> for PaneAgentState {
    fn from(status: PaneAgentStatus) -> Self {
        match status {
            PaneAgentStatus::Busy => Self::Busy,
            PaneAgentStatus::Seen => Self::Seen,
            PaneAgentStatus::Unseen => Self::Unseen,
        }
    }
}

impl PaneAgent {
    const fn record_visible_activity(&mut self, now: Instant) -> bool {
        self.last_visible_activity = now;
        self.mark_visible_activity()
    }

    fn mark_quiet_if_due(&mut self, now: Instant, focused: bool) -> bool {
        self.mark_quiet(now.saturating_duration_since(self.last_visible_activity), focused)
    }

    const fn mark_visible_activity(&mut self) -> bool {
        match self.status {
            PaneAgentStatus::Busy => false,
            PaneAgentStatus::Seen | PaneAgentStatus::Unseen => {
                self.status = PaneAgentStatus::Busy;
                true
            }
        }
    }

    fn mark_quiet(&mut self, quiet_for: Duration, focused: bool) -> bool {
        if self.status != PaneAgentStatus::Busy {
            return false;
        }
        if quiet_for < self::agent_quiet_attention_threshold(self.agent) {
            return false;
        }

        self.status = if focused {
            PaneAgentStatus::Seen
        } else {
            PaneAgentStatus::Unseen
        };
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneUserInteraction {
    MayEcho,
    StartsAgentWork,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaneAgentDetection {
    Agent { pane_id: PaneId, agent: Agent },
    NoAgent { pane_id: PaneId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProcessSnapshot {
    agent: Option<Agent>,
    parent: Option<u32>,
    pid: u32,
}

struct PaneAgentDetectionRequest {
    id: u64,
    shell_processes: Vec<(PaneId, Option<u32>)>,
}

struct PaneAgentDetectionResponse {
    detected_agents: Vec<PaneAgentDetection>,
    id: u64,
}

impl Default for PaneAgentDetector {
    fn default() -> Self {
        Self { system: System::new() }
    }
}

impl PaneAgentDetector {
    fn detect_pane_agents(&mut self, shell_processes: &[(PaneId, Option<u32>)]) -> Vec<PaneAgentDetection> {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );
        let processes = self::process_snapshots(&self.system);
        // Agent detection scans processes on a short attention tick; build scan-wide indexes once so extra panes do not
        // multiply parent-map allocation on the attached client event loop.
        let parent_by_pid = self::parent_by_pid(&processes);
        shell_processes
            .iter()
            .map(|(pane_id, shell_pid)| {
                let Some(shell_pid) = shell_pid else {
                    return PaneAgentDetection::NoAgent { pane_id: *pane_id };
                };
                let Some(agent) = self::detect_descendant_agent(&processes, &parent_by_pid, *shell_pid) else {
                    return PaneAgentDetection::NoAgent { pane_id: *pane_id };
                };
                PaneAgentDetection::Agent {
                    pane_id: *pane_id,
                    agent,
                }
            })
            .collect()
    }
}

impl Default for PaneAgentDetectionWorker {
    fn default() -> Self {
        let (request_sender, request_receiver) = mpsc::channel::<PaneAgentDetectionRequest>();
        let (response_sender, response_receiver) = mpsc::channel::<PaneAgentDetectionResponse>();
        let _detector_thread = thread::spawn(move || {
            let mut detector = PaneAgentDetector::default();
            while let Ok(request) = request_receiver.recv() {
                let detected_agents = detector.detect_pane_agents(&request.shell_processes);
                if response_sender
                    .send(PaneAgentDetectionResponse {
                        detected_agents,
                        id: request.id,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        Self {
            next_request_id: 0,
            pending_request_id: None,
            request_sender,
            response_receiver,
        }
    }
}

impl PaneAgentDetection {
    pub const fn pane_id(&self) -> &PaneId {
        match self {
            Self::Agent { pane_id, .. } | Self::NoAgent { pane_id } => pane_id,
        }
    }

    pub const fn agent(&self) -> Option<Agent> {
        match self {
            Self::Agent { agent, .. } => Some(*agent),
            Self::NoAgent { .. } => None,
        }
    }
}

pub fn detected_agents_refresh_due(refreshed_at: Option<Instant>, now: Instant) -> bool {
    refreshed_at.is_none_or(|refreshed_at| now.saturating_duration_since(refreshed_at) >= AGENT_ATTENTION_POLL_INTERVAL)
}

pub fn runtime_cmd_labels(detected_agents: &[PaneAgentDetection]) -> Vec<(PaneId, Option<String>)> {
    detected_agents
        .iter()
        .filter_map(|detection| {
            detection
                .agent()
                .map(|agent| (*detection.pane_id(), Some(agent.short_name().to_owned())))
        })
        .collect()
}

impl PaneAgentDetectionWorker {
    pub fn request(&mut self, shell_processes: Vec<(PaneId, Option<u32>)>) -> rootcause::Result<()> {
        if self.pending_request_id.is_some() {
            return Ok(());
        }

        let id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or_else(|| rootcause::report!("muxr pane agent detection worker request id overflowed"))?;
        self.request_sender
            .send(PaneAgentDetectionRequest { id, shell_processes })
            .map_err(|error| {
                rootcause::report!("muxr pane agent detection worker stopped").attach(format!("{error}"))
            })?;
        self.pending_request_id = Some(id);
        Ok(())
    }

    pub fn take_finished(&mut self) -> Option<Vec<PaneAgentDetection>> {
        let mut latest = None;
        while let Ok(response) = self.response_receiver.try_recv() {
            if self.pending_request_id == Some(response.id) {
                self.pending_request_id = None;
            }
            latest = Some(response.detected_agents);
        }
        latest
    }

    pub const fn has_pending(&self) -> bool {
        self.pending_request_id.is_some()
    }
}

impl PaneAgents {
    pub fn sync_attention(
        &mut self,
        layout: &SessionLayout,
        detected_agents: &[PaneAgentDetection],
        visible_activity_panes: &[PaneId],
        now: Instant,
    ) -> rootcause::Result<bool> {
        self.retain_layout_panes(layout);
        self.discard_stale_user_interactions(now);
        let mut changed = self.sync_detections(layout, detected_agents, now);
        changed |= self.record_visible_activities(layout, detected_agents, visible_activity_panes, now);
        changed |= self.mark_quiet_agents(layout, now)?;
        Ok(changed)
    }

    pub fn attention_pane_ids(&self, layout: &SessionLayout) -> Vec<PaneId> {
        layout
            .panes()
            .into_iter()
            .filter(|pane| self.needs_attention(pane.id))
            .map(|pane| pane.id)
            .collect()
    }

    fn sync_detections(
        &mut self,
        layout: &SessionLayout,
        detected_agents: &[PaneAgentDetection],
        now: Instant,
    ) -> bool {
        self.retain_layout_panes(layout);
        let pane_ids = self::layout_pane_ids(layout);
        let mut changed = false;
        for pane_id in pane_ids {
            let agent = self::agent_for(detected_agents, pane_id);
            changed |= self.sync_agent_detection(pane_id, agent, now);
        }
        changed
    }

    fn record_visible_activities(
        &mut self,
        layout: &SessionLayout,
        detected_agents: &[PaneAgentDetection],
        visible_activity_panes: &[PaneId],
        now: Instant,
    ) -> bool {
        let mut changed = false;
        for pane_id in visible_activity_panes {
            let agent = self::agent_for(detected_agents, *pane_id);
            if layout.pane(*pane_id).is_none() {
                continue;
            }
            changed |= self.record_visible_activity(*pane_id, agent, now);
        }
        changed
    }

    fn mark_quiet_agents(&mut self, layout: &SessionLayout, now: Instant) -> rootcause::Result<bool> {
        let focused_pane = layout.active_pane_id()?;
        let pane_ids = self::layout_pane_ids(layout);
        let mut changed = false;
        for pane_id in pane_ids {
            changed |= self.mark_quiet_if_due(pane_id, now, pane_id == focused_pane);
        }
        Ok(changed)
    }

    fn retain_layout_panes(&mut self, layout: &SessionLayout) {
        let pane_ids = self::layout_pane_ids(layout).into_iter().collect();
        self.retain_panes(&pane_ids);
    }

    fn sync_agent_detection(&mut self, pane_id: PaneId, agent: Option<Agent>, now: Instant) -> bool {
        let Some(agent) = agent else {
            let changed = self.by_pane.remove(&pane_id).is_some();
            if changed {
                // Agent exit ends the current agent lifecycle, including any pending local-echo suppression.
                self.recent_user_interaction_by_pane.remove(&pane_id);
            }
            return changed;
        };

        match self.by_pane.get_mut(&pane_id) {
            Some(pane_agent) if pane_agent.agent == agent => false,
            Some(pane_agent) => {
                // A new detected agent in the same pane must not inherit activity, attention, or echo suppression
                // from the old process.
                self.recent_user_interaction_by_pane.remove(&pane_id);
                pane_agent.agent = agent;
                pane_agent.last_visible_activity = now;
                pane_agent.status = PaneAgentStatus::Busy;
                true
            }
            None => {
                // A newly observed agent starts Busy even if its first dirty frame was missed by the sampled scan.
                // Pre-agent input suppression belongs to the shell and must not hide the new agent's output.
                self.recent_user_interaction_by_pane.remove(&pane_id);
                self.by_pane.insert(
                    pane_id,
                    PaneAgent {
                        agent,
                        last_visible_activity: now,
                        status: PaneAgentStatus::Busy,
                    },
                );
                true
            }
        }
    }

    pub fn record_user_interaction(&mut self, pane_id: PaneId, interaction: PaneUserInteraction, now: Instant) {
        match interaction {
            PaneUserInteraction::MayEcho => {
                self.recent_user_interaction_by_pane.insert(pane_id, now);
            }
            PaneUserInteraction::StartsAgentWork => {
                // Submitting a prompt is user input, but the following redraw is the agent starting work. Clear prior
                // typing suppression so a fast response is not lost as if it were only local echo.
                self.recent_user_interaction_by_pane.remove(&pane_id);
            }
        }
    }

    fn record_visible_activity(&mut self, pane_id: PaneId, agent: Option<Agent>, now: Instant) -> bool {
        if self.recent_user_interaction_by_pane.contains_key(&pane_id) {
            // User typing and mouse gestures can redraw through the PTY. Those bytes still render, but they are not
            // agent work and must not flip agent attention back to Busy.
            return false;
        }
        let Some(agent) = agent else {
            // PTY output before process detection is still rendered, but it is not agent activity yet. Newly detected
            // agents start Busy instead of inheriting stale shell output from before the scan observed them.
            return false;
        };
        let Some(pane_agent) = self.by_pane.get_mut(&pane_id) else {
            return false;
        };
        if pane_agent.agent != agent {
            return false;
        }

        pane_agent.record_visible_activity(now)
    }

    fn mark_quiet_if_due(&mut self, pane_id: PaneId, now: Instant, focused: bool) -> bool {
        let Some(pane_agent) = self.by_pane.get_mut(&pane_id) else {
            return false;
        };
        pane_agent.mark_quiet_if_due(now, focused)
    }

    pub fn acknowledge_attention(&mut self, pane_id: PaneId) -> bool {
        if !self.needs_attention(pane_id) {
            return false;
        }
        self.set_status(pane_id, PaneAgentStatus::Seen);
        true
    }

    fn needs_attention(&self, pane_id: PaneId) -> bool {
        self.by_pane
            .get(&pane_id)
            .is_some_and(|pane_agent| pane_agent.status == PaneAgentStatus::Unseen)
    }

    pub fn snapshot_states(&self) -> Vec<(PaneId, PaneAgentState)> {
        self.by_pane
            .iter()
            .map(|(pane_id, pane_agent)| (*pane_id, pane_agent.status.into()))
            .collect()
    }

    fn retain_panes(&mut self, pane_ids: &BTreeSet<PaneId>) {
        self.by_pane.retain(|pane_id, _pane_agent| pane_ids.contains(pane_id));
        self.recent_user_interaction_by_pane
            .retain(|pane_id, _last_activity| pane_ids.contains(pane_id));
    }

    fn discard_stale_user_interactions(&mut self, now: Instant) {
        self.recent_user_interaction_by_pane.retain(|_pane_id, last_activity| {
            now.saturating_duration_since(*last_activity) <= USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION
        });
    }

    fn set_status(&mut self, pane_id: PaneId, status: PaneAgentStatus) {
        if let Some(pane_agent) = self.by_pane.get_mut(&pane_id) {
            pane_agent.status = status;
        }
    }
}

fn layout_pane_ids(layout: &SessionLayout) -> Vec<PaneId> {
    layout.panes().into_iter().map(|pane| pane.id).collect()
}

fn agent_for(detected_agents: &[PaneAgentDetection], pane_id: PaneId) -> Option<Agent> {
    detected_agents
        .iter()
        .find(|detection| *detection.pane_id() == pane_id)
        .and_then(PaneAgentDetection::agent)
}

#[expect(clippy::match_same_arms, reason = "S8e keeps per-agent threshold tuning exhaustive")]
const fn agent_quiet_attention_threshold(agent: Agent) -> Duration {
    match agent {
        Agent::Claude => Duration::from_secs(3),
        Agent::Codex => Duration::from_secs(3),
        Agent::Cursor => Duration::from_secs(3),
        Agent::Gemini => Duration::from_secs(3),
        Agent::Opencode => Duration::from_secs(3),
    }
}

fn process_snapshots(system: &System) -> Vec<ProcessSnapshot> {
    system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessSnapshot {
            agent: self::detect_agent_process(process),
            parent: process.parent().map(Pid::as_u32),
            pid: pid.as_u32(),
        })
        .collect()
}

fn parent_by_pid(processes: &[ProcessSnapshot]) -> HashMap<u32, Option<u32>> {
    processes.iter().map(|process| (process.pid, process.parent)).collect()
}

fn detect_agent_process(process: &Process) -> Option<Agent> {
    // Muxr intentionally supports direct agent executables only. Do not scan arbitrary argv text:
    // commands such as `rg codex` mention an agent but are not agent processes.
    process
        .cmd()
        .first()
        .and_then(|cmd| self::detect_agent_command_name(cmd))
        .or_else(|| self::detect_agent_command_name(process.name()))
}

fn detect_agent_command_name(command: &OsStr) -> Option<Agent> {
    Path::new(command)
        .file_name()
        .and_then(OsStr::to_str)
        // Process scanning supports direct agent executables only. Use exact basename parsing here because
        // the shared fuzzy detector is for human command text and would classify wrappers such as `rg-codex`.
        .and_then(|basename| match basename {
            // Cursor's direct executable is `cursor-agent`, while the canonical agent id stays `cursor`.
            "cursor-agent" => Some(Agent::Cursor),
            basename => Agent::from_name(basename).ok(),
        })
}

fn detect_descendant_agent(
    processes: &[ProcessSnapshot],
    parent_by_pid: &HashMap<u32, Option<u32>>,
    root_pid: u32,
) -> Option<Agent> {
    let mut best: Option<(usize, Agent)> = None;
    for process in processes {
        let Some(depth) = self::descendant_depth(parent_by_pid, process.pid, root_pid) else {
            continue;
        };
        if depth == 0 {
            continue;
        }
        let Some(agent) = process.agent else {
            continue;
        };
        if best.as_ref().is_none_or(|(best_depth, best_agent)| {
            depth < *best_depth || (depth == *best_depth && agent.priority() < best_agent.priority())
        }) {
            best = Some((depth, agent));
        }
    }
    best.map(|(_depth, agent)| agent)
}

fn descendant_depth(parent_by_pid: &HashMap<u32, Option<u32>>, pid: u32, root_pid: u32) -> Option<usize> {
    let mut current = pid;
    for depth in 0..64 {
        if current == root_pid {
            return Some(depth);
        }
        current = parent_by_pid.get(&current).copied().flatten()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use muxr_core::SessionName;

    use super::*;
    use crate::pane_split::PaneSplitAxis;
    use crate::state::SessionMetadata;

    fn pane_agent_status(pane_agents: &PaneAgents, pane_id: PaneId) -> PaneAgentState {
        pane_agents
            .by_pane
            .get(&pane_id)
            .map_or(PaneAgentState::NoAgent, |pane_agent| pane_agent.status.into())
    }

    #[test]
    fn test_sync_agent_detection_when_agent_is_detected_marks_busy() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        let now = Instant::now();

        assert2::assert!(pane_agents.sync_agent_detection(pane_id, Some(Agent::Codex), now));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        Ok(())
    }

    #[test]
    fn test_sync_agent_detection_when_agent_exits_clears_state() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        pane_agents.sync_agent_detection(pane_id, Some(Agent::Codex), Instant::now());
        pane_agents.set_status(pane_id, PaneAgentStatus::Unseen);

        assert2::assert!(pane_agents.sync_agent_detection(pane_id, None, Instant::now()));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::NoAgent);
        Ok(())
    }

    #[test]
    fn test_sync_agent_detection_when_new_agent_is_detected_clears_prior_echo_suppression() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        pane_agents.record_user_interaction(pane_id, PaneUserInteraction::MayEcho, then);

        assert2::assert!(pane_agents.sync_agent_detection(
            pane_id,
            Some(Agent::Codex),
            then + Duration::from_millis(100)
        ));
        pane_agents.set_status(pane_id, PaneAgentStatus::Seen);

        assert2::assert!(pane_agents.record_visible_activity(
            pane_id,
            Some(Agent::Codex),
            then + Duration::from_millis(150),
        ));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        Ok(())
    }

    #[test]
    fn test_sync_agent_detection_when_agent_is_replaced_clears_prior_echo_suppression() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        assert2::assert!(pane_agents.sync_agent_detection(pane_id, Some(Agent::Codex), then));
        pane_agents.record_user_interaction(pane_id, PaneUserInteraction::MayEcho, then + Duration::from_millis(100));

        assert2::assert!(pane_agents.sync_agent_detection(
            pane_id,
            Some(Agent::Cursor),
            then + Duration::from_millis(150)
        ));
        pane_agents.set_status(pane_id, PaneAgentStatus::Seen);

        assert2::assert!(pane_agents.record_visible_activity(
            pane_id,
            Some(Agent::Cursor),
            then + Duration::from_millis(200),
        ));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        Ok(())
    }

    #[test]
    fn test_sync_agent_detection_when_busy_agent_is_still_running_keeps_busy() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        let now = Instant::now();

        assert2::assert!(pane_agents.sync_agent_detection(pane_id, Some(Agent::Codex), now));
        assert2::assert!(!pane_agents.sync_agent_detection(pane_id, Some(Agent::Codex), now));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        Ok(())
    }

    #[rstest::rstest]
    #[case::claude(Agent::Claude)]
    #[case::codex(Agent::Codex)]
    #[case::cursor(Agent::Cursor)]
    #[case::gemini(Agent::Gemini)]
    #[case::opencode(Agent::Opencode)]
    fn test_agent_quiet_attention_threshold_when_agent_is_supported_returns_default(#[case] agent: Agent) {
        pretty_assertions::assert_eq!(agent_quiet_attention_threshold(agent), Duration::from_secs(3));
    }

    #[rstest::rstest]
    #[case::codex(Agent::Codex, "cx")]
    #[case::cursor(Agent::Cursor, "cu")]
    fn test_runtime_cmd_labels_when_agent_is_detected_uses_short_name(
        #[case] agent: Agent,
        #[case] expected_label: &str,
    ) -> rootcause::Result<()> {
        let pane_id = self::pane_id()?;

        pretty_assertions::assert_eq!(
            self::runtime_cmd_labels(&[PaneAgentDetection::Agent { pane_id, agent }]),
            vec![(pane_id, Some(expected_label.to_owned()))],
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::never(None, true)]
    #[case::recent(Some(Duration::from_millis(1)), false)]
    #[case::elapsed(Some(AGENT_ATTENTION_POLL_INTERVAL), true)]
    fn test_detected_agents_refresh_due_when_last_refresh_varies(
        #[case] refreshed_age: Option<Duration>,
        #[case] expected: bool,
    ) {
        let now = Instant::now();
        let refreshed_at = refreshed_age.map(|age| now.checked_sub(age).expect("expected valid refresh age"));

        pretty_assertions::assert_eq!(self::detected_agents_refresh_due(refreshed_at, now), expected);
    }

    #[test]
    fn test_record_visible_activity_when_unseen_agent_has_output_marks_busy() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        pane_agents.sync_agent_detection(pane_id, Some(Agent::Codex), Instant::now());
        pane_agents.set_status(pane_id, PaneAgentStatus::Unseen);

        assert2::assert!(pane_agents.record_visible_activity(pane_id, Some(Agent::Codex), Instant::now()));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        Ok(())
    }

    #[test]
    fn test_mark_agent_quiet_when_unfocused_busy_agent_is_quiet_marks_unseen() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        pane_agents.sync_agent_detection(pane_id, Some(Agent::Codex), then);

        assert2::assert!(pane_agents.mark_quiet_if_due(pane_id, then + Duration::from_secs(3), false));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Unseen);
        Ok(())
    }

    #[test]
    fn test_mark_agent_quiet_when_focused_busy_agent_is_quiet_marks_seen() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        pane_agents.sync_agent_detection(pane_id, Some(Agent::Codex), then);

        assert2::assert!(pane_agents.mark_quiet_if_due(pane_id, then + Duration::from_secs(3), true));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_acknowledge_agent_attention_when_agent_is_unseen_marks_agent_seen() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        pane_agents.sync_agent_detection(pane_id, Some(Agent::Codex), Instant::now());
        pane_agents.set_status(pane_id, PaneAgentStatus::Unseen);

        assert2::assert!(pane_agents.acknowledge_attention(pane_id));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_attention_pane_ids_when_agent_is_unseen_returns_pane() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(1)?;
        let detected_agents = self::detected_agents(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], then)?);
        assert2::assert!(!pane_agents.sync_attention(
            &layout,
            &detected_agents,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(1))?
        )?);
        assert2::assert!(pane_agents.sync_attention(
            &layout,
            &detected_agents,
            &[],
            self::instant_after(then, Duration::from_secs(4))?
        )?);

        pretty_assertions::assert_eq!(pane_agents.attention_pane_ids(&layout), vec![pane_id]);
        Ok(())
    }

    #[test]
    fn test_sync_attention_when_agent_has_visible_activity_and_then_is_quiet_unfocused_marks_unseen()
    -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(1)?;
        let detected_agents = self::detected_agents(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], then)?);
        assert2::assert!(!pane_agents.sync_attention(
            &layout,
            &detected_agents,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(1))?
        )?);
        assert2::assert!(pane_agents.sync_attention(
            &layout,
            &detected_agents,
            &[],
            self::instant_after(then, Duration::from_secs(4))?
        )?);

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Unseen);
        Ok(())
    }

    #[test]
    fn test_sync_attention_when_agent_detection_lags_visible_activity_starts_busy() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(1)?;
        let no_agent = self::detected_agents(1, None)?;
        let detected_agents = self::detected_agents(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(!pane_agents.sync_attention(&layout, &no_agent, std::slice::from_ref(&pane_id), then,)?);
        let detected_at = self::instant_after(then, Duration::from_millis(100))?;
        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], detected_at)?);

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        let quiet_at = self::instant_after(then, Duration::from_secs(4))?;
        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], quiet_at)?);
        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Unseen);
        Ok(())
    }

    #[test]
    fn test_sync_attention_when_newly_detected_unfocused_agent_is_quiet_marks_unseen() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(1)?;
        let detected_agents = self::detected_agents(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], then)?);
        assert2::assert!(pane_agents.sync_attention(
            &layout,
            &detected_agents,
            &[],
            self::instant_after(then, Duration::from_secs(3))?
        )?);

        pretty_assertions::assert_eq!(pane_agents.attention_pane_ids(&layout), vec![pane_id]);
        Ok(())
    }

    #[rstest::rstest]
    #[case::recent_user_interaction(Duration::from_millis(100), PaneAgentState::Seen, false)]
    #[case::stale_user_interaction(Duration::from_millis(501), PaneAgentState::Busy, true)]
    fn test_sync_attention_when_user_interaction_echoes_visible_activity_does_not_mark_agent_busy(
        #[case] visible_activity_delay: Duration,
        #[case] expected_agent_status: PaneAgentState,
        #[case] expected_changed: bool,
    ) -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(2)?;
        let detected_agents = self::detected_agents(2, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], then)?);
        let seen_at = self::instant_after(then, Duration::from_secs(3))?;
        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], seen_at)?);
        pane_agents.record_user_interaction(pane_id, PaneUserInteraction::MayEcho, seen_at);

        let changed = pane_agents.sync_attention(
            &layout,
            &detected_agents,
            std::slice::from_ref(&pane_id),
            self::instant_after(seen_at, visible_activity_delay)?,
        )?;

        pretty_assertions::assert_eq!(changed, expected_changed);
        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), expected_agent_status);
        Ok(())
    }

    #[test]
    fn test_sync_attention_when_prompt_submit_follows_typing_keeps_fast_agent_activity() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(2)?;
        let detected_agents = self::detected_agents(2, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], then)?);
        let seen_at = self::instant_after(then, Duration::from_secs(3))?;
        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], seen_at)?);
        pane_agents.record_user_interaction(pane_id, PaneUserInteraction::MayEcho, seen_at);
        pane_agents.record_user_interaction(
            pane_id,
            PaneUserInteraction::StartsAgentWork,
            self::instant_after(seen_at, Duration::from_millis(100))?,
        );
        assert2::assert!(pane_agents.sync_attention(
            &layout,
            &detected_agents,
            std::slice::from_ref(&pane_id),
            self::instant_after(seen_at, Duration::from_millis(150))?,
        )?);

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        Ok(())
    }

    #[test]
    fn test_sync_attention_when_mouse_interaction_redraws_does_not_mark_agent_busy() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(2)?;
        let detected_agents = self::detected_agents(2, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], then)?);
        let seen_at = self::instant_after(then, Duration::from_secs(3))?;
        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], seen_at)?);
        pane_agents.record_user_interaction(pane_id, PaneUserInteraction::MayEcho, seen_at);
        assert2::assert!(!pane_agents.sync_attention(
            &layout,
            &detected_agents,
            std::slice::from_ref(&pane_id),
            self::instant_after(seen_at, Duration::from_millis(100))?,
        )?);

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_sync_attention_when_agent_identity_changes_resets_activity_and_status() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(1)?;
        let codex = self::detected_agents(1, Some(Agent::Codex))?;
        let cursor = self::detected_agents(1, Some(Agent::Cursor))?;
        let then = Instant::now();

        assert2::assert!(pane_agents.sync_attention(&layout, &codex, &[], then)?);
        assert2::assert!(!pane_agents.sync_attention(
            &layout,
            &codex,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_millis(100))?,
        )?);
        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);

        assert2::assert!(pane_agents.sync_attention(
            &layout,
            &cursor,
            &[],
            self::instant_after(then, Duration::from_millis(200))?,
        )?);
        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        assert2::assert!(pane_agents.sync_attention(
            &layout,
            &cursor,
            &[],
            self::instant_after(then, Duration::from_secs(4))?,
        )?);
        pretty_assertions::assert_eq!(pane_agents.attention_pane_ids(&layout), vec![pane_id]);
        Ok(())
    }

    #[test]
    fn test_sync_attention_when_fresh_tracker_observes_running_agent_starts_busy() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(1)?;
        let detected_agents = self::detected_agents(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], then)?);

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        pretty_assertions::assert_eq!(pane_agents.attention_pane_ids(&layout), Vec::<PaneId>::new());
        Ok(())
    }

    #[test]
    fn test_sync_attention_when_visible_activity_is_quiet_and_focused_marks_seen() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(2)?;
        let detected_agents = self::detected_agents(2, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], then)?);
        assert2::assert!(!pane_agents.sync_attention(
            &layout,
            &detected_agents,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(1))?
        )?);
        assert2::assert!(!pane_agents.sync_attention(
            &layout,
            &detected_agents,
            &[],
            self::instant_after(then, Duration::from_secs(2))?
        )?);
        assert2::assert!(pane_agents.sync_attention(
            &layout,
            &detected_agents,
            &[],
            self::instant_after(then, Duration::from_secs(4))?
        )?);

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_sync_attention_when_unseen_agent_has_visible_activity_marks_busy() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(1)?;
        let detected_agents = self::detected_agents(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], then)?);
        assert2::assert!(!pane_agents.sync_attention(
            &layout,
            &detected_agents,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(1))?,
        )?);
        assert2::assert!(pane_agents.sync_attention(
            &layout,
            &detected_agents,
            &[],
            self::instant_after(then, Duration::from_secs(4))?,
        )?);

        assert2::assert!(pane_agents.sync_attention(
            &layout,
            &detected_agents,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(5))?,
        )?);

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        Ok(())
    }

    #[test]
    fn test_sync_attention_when_shell_output_is_quiet_does_not_mark_attention() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(1)?;
        let detected_agents = self::detected_agents(1, None)?;
        let then = Instant::now();

        assert2::assert!(!pane_agents.sync_attention(
            &layout,
            &detected_agents,
            std::slice::from_ref(&pane_id),
            then,
        )?);
        assert2::assert!(!pane_agents.sync_attention(
            &layout,
            &detected_agents,
            &[],
            self::instant_after(then, Duration::from_secs(3))?
        )?);

        pretty_assertions::assert_eq!(pane_agents.attention_pane_ids(&layout), Vec::<PaneId>::new());
        Ok(())
    }

    #[test]
    fn test_sync_attention_when_agent_exits_clears_agent_status() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(1)?;
        let detected_agents = self::detected_agents(1, Some(Agent::Codex))?;
        let no_agent = self::detected_agents(1, None)?;
        let then = Instant::now();

        assert2::assert!(pane_agents.sync_attention(&layout, &detected_agents, &[], then)?);
        assert2::assert!(!pane_agents.sync_attention(
            &layout,
            &detected_agents,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(1))?
        )?);
        assert2::assert!(pane_agents.sync_attention(
            &layout,
            &detected_agents,
            &[],
            self::instant_after(then, Duration::from_secs(4))?
        )?);

        assert2::assert!(pane_agents.sync_attention(
            &layout,
            &no_agent,
            &[],
            self::instant_after(then, Duration::from_secs(5))?
        )?);

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::NoAgent);
        pretty_assertions::assert_eq!(pane_agents.attention_pane_ids(&layout), Vec::<PaneId>::new());
        Ok(())
    }

    #[test]
    fn test_detect_descendant_agent_when_child_cmd_contains_agent_returns_agent() {
        let processes = vec![
            self::process(1, None, None),
            self::process(2, Some(1), Some(Agent::Codex)),
        ];

        pretty_assertions::assert_eq!(self::detect_agent(&processes, 1), Some(Agent::Codex));
    }

    #[test]
    fn test_detect_descendant_agent_when_only_shell_matches_agent_ignores_shell() {
        let processes = vec![self::process(1, None, Some(Agent::Codex))];

        pretty_assertions::assert_eq!(self::detect_agent(&processes, 1), None);
    }

    #[test]
    fn test_detect_descendant_agent_when_agent_process_exits_returns_none() {
        let processes = vec![self::process(1, None, None), self::process(2, Some(1), None)];

        pretty_assertions::assert_eq!(self::detect_agent(&processes, 1), None);
    }

    #[test]
    fn test_detect_descendant_agent_prefers_nearest_agent_process() {
        let processes = vec![
            self::process(1, None, None),
            self::process(2, Some(1), Some(Agent::Codex)),
            self::process(3, Some(2), Some(Agent::Codex)),
        ];

        pretty_assertions::assert_eq!(self::detect_agent(&processes, 1), Some(Agent::Codex));
    }

    #[rstest::rstest]
    #[case::codex_path("/opt/homebrew/bin/codex", Some(Agent::Codex))]
    #[case::codex_name("codex", Some(Agent::Codex))]
    #[case::cursor_agent_name("cursor-agent", Some(Agent::Cursor))]
    #[case::plain_non_agent("rg", None)]
    #[case::agent_name_in_wrapper("rg-codex", None)]
    #[case::agent_name_in_other_name("notcodex", None)]
    fn test_detect_agent_command_name_when_basename_varies_returns_exact_agent(
        #[case] command: &str,
        #[case] expected: Option<Agent>,
    ) {
        pretty_assertions::assert_eq!(detect_agent_command_name(OsStr::new(command)), expected);
    }

    fn pane_id() -> rootcause::Result<PaneId> {
        PaneId::new(1)
    }

    fn layout() -> rootcause::Result<SessionLayout> {
        let session: SessionName = "work".parse()?;
        let mut layout = SessionLayout::initial(&session, self::metadata("sh", 1))?;
        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        Ok(layout)
    }

    fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }

    fn detected_agents(pane_id: u32, agent: Option<Agent>) -> rootcause::Result<Vec<PaneAgentDetection>> {
        let pane_id = PaneId::new(pane_id)?;
        Ok(vec![agent.map_or(PaneAgentDetection::NoAgent { pane_id }, |agent| {
            PaneAgentDetection::Agent { pane_id, agent }
        })])
    }

    fn instant_after(instant: Instant, duration: Duration) -> rootcause::Result<Instant> {
        instant
            .checked_add(duration)
            .ok_or_else(|| rootcause::report!("test instant overflowed"))
    }

    fn process(pid: u32, parent: Option<u32>, agent: Option<Agent>) -> ProcessSnapshot {
        ProcessSnapshot { agent, parent, pid }
    }

    fn detect_agent(processes: &[ProcessSnapshot], root_pid: u32) -> Option<Agent> {
        let parent_by_pid = parent_by_pid(processes);
        detect_descendant_agent(processes, &parent_by_pid, root_pid)
    }
}
