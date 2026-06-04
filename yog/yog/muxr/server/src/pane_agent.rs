use std::collections::BTreeSet;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
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

const PENDING_VISIBLE_ACTIVITY_RETENTION: Duration = Duration::from_millis(500);
const USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub struct PaneAgentProcessScanner {
    system: System,
}

#[derive(Debug, Default)]
pub struct PaneAgentRuntime {
    agents: HashMap<PaneId, Agent>,
    last_visible_activity: HashMap<PaneId, Instant>,
    pending_visible_activity: HashMap<PaneId, Instant>,
    recent_user_interaction: HashMap<PaneId, Instant>,
    states: HashMap<PaneId, PaneAgentState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneUserInteraction {
    MayEcho,
    StartsAgentWork,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaneAgentProcess {
    Agent { pane_id: PaneId, agent: Agent },
    NoAgent { pane_id: PaneId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentIdentitySync {
    Cleared,
    FirstObservation,
    Replaced,
    Same,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProcessSnapshot {
    agent: Option<Agent>,
    parent: Option<u32>,
    pid: u32,
}

impl Default for PaneAgentProcessScanner {
    fn default() -> Self {
        Self { system: System::new() }
    }
}

impl PaneAgentProcessScanner {
    pub fn detect_pane_agents(&mut self, shell_processes: &[(PaneId, Option<u32>)]) -> Vec<PaneAgentProcess> {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );
        let processes = self::process_snapshots(&self.system);
        // Process scanning runs on a short attention tick; build scan-wide indexes once so extra panes do not multiply
        // parent-map allocation on the attached client event loop.
        let parent_by_pid = self::parent_by_pid(&processes);
        shell_processes
            .iter()
            .map(|(pane_id, shell_pid)| {
                let Some(shell_pid) = shell_pid else {
                    return PaneAgentProcess::NoAgent { pane_id: *pane_id };
                };
                let Some(agent) = self::detect_descendant_agent(&processes, &parent_by_pid, *shell_pid) else {
                    return PaneAgentProcess::NoAgent { pane_id: *pane_id };
                };
                PaneAgentProcess::Agent {
                    pane_id: *pane_id,
                    agent,
                }
            })
            .collect()
    }
}

impl PaneAgentProcess {
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

impl PaneAgentRuntime {
    pub fn sync_process(&mut self, pane_id: PaneId, agent: Option<Agent>) -> bool {
        let identity_sync = self.sync_identity(pane_id, agent);
        if identity_sync == AgentIdentitySync::Replaced {
            self.set_seen(pane_id);
        }
        let mut changed = self.sync_process_state(pane_id, agent);
        changed |= identity_sync != AgentIdentitySync::Same;
        if agent.is_none() {
            self.last_visible_activity.remove(&pane_id);
            self.pending_visible_activity.remove(&pane_id);
        }
        changed
    }

    pub fn record_user_interaction(&mut self, pane_id: PaneId, interaction: PaneUserInteraction, now: Instant) {
        match interaction {
            PaneUserInteraction::MayEcho => {
                self.recent_user_interaction.insert(pane_id, now);
            }
            PaneUserInteraction::StartsAgentWork => {
                // Submitting a prompt is user input, but the following redraw is the agent starting work. Clear prior
                // typing suppression so a fast response is not lost as if it were only local echo.
                self.recent_user_interaction.remove(&pane_id);
            }
        }
    }

    pub fn record_visible_activity(&mut self, pane_id: PaneId, agent: Option<Agent>, now: Instant) -> bool {
        if self.recent_user_interaction.contains_key(&pane_id) {
            // User typing and mouse gestures can redraw through the PTY. Those bytes still render, but they are not
            // agent work and must not flip agent attention back to Busy.
            return false;
        }
        if agent.is_none() {
            // PTY output can arrive before the throttled process scan observes a just-started agent.
            // Keep only a short-lived timestamp so shell output cannot become stale agent activity later.
            self.pending_visible_activity.insert(pane_id, now);
            return false;
        }

        self.pending_visible_activity.remove(&pane_id);
        self.last_visible_activity.insert(pane_id, now);
        self.mark_visible_activity(pane_id)
    }

    pub fn consume_pending_visible_activity(&mut self, pane_id: PaneId, agent: Option<Agent>) -> bool {
        if agent.is_none() {
            return false;
        }
        let Some(last_visible_activity) = self.pending_visible_activity.remove(&pane_id) else {
            return false;
        };
        self.last_visible_activity.insert(pane_id, last_visible_activity);
        self.mark_visible_activity(pane_id)
    }

    pub fn mark_quiet_if_due(&mut self, pane_id: PaneId, agent: Agent, now: Instant, focused: bool) -> bool {
        let Some(last_visible_activity) = self.last_visible_activity.get(&pane_id) else {
            return false;
        };
        self.mark_quiet(
            pane_id,
            agent,
            now.saturating_duration_since(*last_visible_activity),
            focused,
        )
    }

    fn sync_identity(&mut self, pane_id: PaneId, agent: Option<Agent>) -> AgentIdentitySync {
        match agent {
            Some(agent) => {
                let previous = self.agents.insert(pane_id, agent);
                if previous == Some(agent) {
                    return AgentIdentitySync::Same;
                }
                if previous.is_some() {
                    // A new agent process in the same pane must not inherit activity or attention from the old one.
                    self.last_visible_activity.remove(&pane_id);
                    self.pending_visible_activity.remove(&pane_id);
                    return AgentIdentitySync::Replaced;
                }
                AgentIdentitySync::FirstObservation
            }
            None => {
                if self.agents.remove(&pane_id).is_some() {
                    AgentIdentitySync::Cleared
                } else {
                    AgentIdentitySync::Same
                }
            }
        }
    }

    fn sync_process_state(&mut self, pane_id: PaneId, agent: Option<Agent>) -> bool {
        match (self.state(pane_id), agent) {
            (PaneAgentState::NoAgent, Some(_agent)) => {
                self.set_state(pane_id, PaneAgentState::Seen);
                true
            }
            (PaneAgentState::NoAgent, None) => false,
            (_, None) => self.states.remove(&pane_id).is_some(),
            (PaneAgentState::Seen | PaneAgentState::Busy | PaneAgentState::Unseen, Some(_agent)) => false,
        }
    }

    pub fn mark_visible_activity(&mut self, pane_id: PaneId) -> bool {
        match self.state(pane_id) {
            PaneAgentState::Busy => false,
            PaneAgentState::NoAgent | PaneAgentState::Seen | PaneAgentState::Unseen => {
                self.set_state(pane_id, PaneAgentState::Busy);
                true
            }
        }
    }

    pub fn mark_quiet(&mut self, pane_id: PaneId, agent: Agent, quiet_for: Duration, focused: bool) -> bool {
        if self.state(pane_id) != PaneAgentState::Busy {
            return false;
        }
        if quiet_for < self::agent_quiet_attention_threshold(agent) {
            return false;
        }

        self.set_state(
            pane_id,
            if focused {
                PaneAgentState::Seen
            } else {
                PaneAgentState::Unseen
            },
        );
        true
    }

    pub fn acknowledge_attention(&mut self, pane_id: PaneId) -> bool {
        if !self.needs_attention(pane_id) {
            return false;
        }
        self.set_state(pane_id, PaneAgentState::Seen);
        true
    }

    pub fn needs_attention(&self, pane_id: PaneId) -> bool {
        matches!(self.state(pane_id), PaneAgentState::Unseen)
    }

    pub fn state(&self, pane_id: PaneId) -> PaneAgentState {
        self.states.get(&pane_id).copied().unwrap_or(PaneAgentState::NoAgent)
    }

    pub fn states(&self) -> Vec<(PaneId, PaneAgentState)> {
        self.states.iter().map(|(pane_id, state)| (*pane_id, *state)).collect()
    }

    pub fn set_seen(&mut self, pane_id: PaneId) {
        self.set_state(pane_id, PaneAgentState::Seen);
    }

    pub fn retain_panes(&mut self, pane_ids: &BTreeSet<PaneId>) {
        self.agents.retain(|pane_id, _agent| pane_ids.contains(pane_id));
        self.last_visible_activity
            .retain(|pane_id, _last_activity| pane_ids.contains(pane_id));
        self.pending_visible_activity
            .retain(|pane_id, _last_activity| pane_ids.contains(pane_id));
        self.recent_user_interaction
            .retain(|pane_id, _last_activity| pane_ids.contains(pane_id));
        self.states.retain(|pane_id, _state| pane_ids.contains(pane_id));
    }

    pub fn discard_stale_activity(&mut self, now: Instant) {
        self.pending_visible_activity.retain(|_pane_id, last_activity| {
            now.saturating_duration_since(*last_activity) <= PENDING_VISIBLE_ACTIVITY_RETENTION
        });
        self.recent_user_interaction.retain(|_pane_id, last_activity| {
            now.saturating_duration_since(*last_activity) <= USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION
        });
    }

    fn set_state(&mut self, pane_id: PaneId, state: PaneAgentState) {
        if state == PaneAgentState::NoAgent {
            self.states.remove(&pane_id);
        } else {
            self.states.insert(pane_id, state);
        }
    }
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
    use super::*;

    #[test]
    fn test_sync_agent_process_when_agent_is_detected_marks_seen() -> rootcause::Result<()> {
        let mut runtime = PaneAgentRuntime::default();
        let pane_id = self::pane_id()?;

        assert2::assert!(runtime.sync_process(pane_id, Some(Agent::Codex)));

        pretty_assertions::assert_eq!(runtime.state(pane_id), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_sync_agent_process_when_agent_exits_clears_state() -> rootcause::Result<()> {
        let mut runtime = PaneAgentRuntime::default();
        let pane_id = self::pane_id()?;
        runtime.set_state(pane_id, PaneAgentState::Unseen);

        assert2::assert!(runtime.sync_process(pane_id, None));

        pretty_assertions::assert_eq!(runtime.state(pane_id), PaneAgentState::NoAgent);
        Ok(())
    }

    #[test]
    fn test_sync_agent_process_when_seen_agent_is_still_running_keeps_seen() -> rootcause::Result<()> {
        let mut runtime = PaneAgentRuntime::default();
        let pane_id = self::pane_id()?;

        assert2::assert!(runtime.sync_process(pane_id, Some(Agent::Codex)));
        assert2::assert!(!runtime.sync_process(pane_id, Some(Agent::Codex)));

        pretty_assertions::assert_eq!(runtime.state(pane_id), PaneAgentState::Seen);
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

    #[test]
    fn test_mark_agent_visible_activity_when_unseen_agent_has_output_marks_busy() -> rootcause::Result<()> {
        let mut runtime = PaneAgentRuntime::default();
        let pane_id = self::pane_id()?;
        runtime.set_state(pane_id, PaneAgentState::Unseen);

        assert2::assert!(runtime.mark_visible_activity(pane_id));

        pretty_assertions::assert_eq!(runtime.state(pane_id), PaneAgentState::Busy);
        Ok(())
    }

    #[test]
    fn test_mark_agent_quiet_when_unfocused_busy_agent_is_quiet_marks_unseen() -> rootcause::Result<()> {
        let mut runtime = PaneAgentRuntime::default();
        let pane_id = self::pane_id()?;
        runtime.set_state(pane_id, PaneAgentState::Busy);

        assert2::assert!(runtime.mark_quiet(pane_id, Agent::Codex, Duration::from_secs(3), false));

        pretty_assertions::assert_eq!(runtime.state(pane_id), PaneAgentState::Unseen);
        Ok(())
    }

    #[test]
    fn test_mark_agent_quiet_when_focused_busy_agent_is_quiet_marks_seen() -> rootcause::Result<()> {
        let mut runtime = PaneAgentRuntime::default();
        let pane_id = self::pane_id()?;
        runtime.set_state(pane_id, PaneAgentState::Busy);

        assert2::assert!(runtime.mark_quiet(pane_id, Agent::Codex, Duration::from_secs(3), true));

        pretty_assertions::assert_eq!(runtime.state(pane_id), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_acknowledge_agent_attention_when_agent_is_unseen_marks_agent_seen() -> rootcause::Result<()> {
        let mut runtime = PaneAgentRuntime::default();
        let pane_id = self::pane_id()?;
        runtime.set_state(pane_id, PaneAgentState::Unseen);

        assert2::assert!(runtime.acknowledge_attention(pane_id));

        pretty_assertions::assert_eq!(runtime.state(pane_id), PaneAgentState::Seen);
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

    fn process(pid: u32, parent: Option<u32>, agent: Option<Agent>) -> ProcessSnapshot {
        ProcessSnapshot { agent, parent, pid }
    }

    fn detect_agent(processes: &[ProcessSnapshot], root_pid: u32) -> Option<Agent> {
        let parent_by_pid = parent_by_pid(processes);
        detect_descendant_agent(processes, &parent_by_pid, root_pid)
    }
}
