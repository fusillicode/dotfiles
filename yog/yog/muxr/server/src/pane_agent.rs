use std::collections::BTreeSet;
use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use muxr_core::PaneAgentState;
use muxr_core::PaneId;
use ytil_agents::agent::Agent;

use crate::pane_cmd::PaneCmdObservation;
use crate::state::SessionLayout;

const USER_INPUT_VISIBLE_ACTIVITY_SUPPRESSION: Duration = Duration::from_millis(500);

#[derive(Debug, Default)]
pub struct PaneAgents {
    by_pane: HashMap<PaneId, PaneAgentLifecycle>,
}

#[derive(Debug)]
struct PaneAgentLifecycle {
    agent: Agent,
    last_visible_activity: Instant,
    pending_work_start_at: Option<Instant>,
    recent_user_interaction: Option<Instant>,
    status: PaneAgentStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneAgentCmdObservation {
    Agent(Agent),
    TrustedNoAgent,
    Unknown,
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

impl PaneAgentLifecycle {
    const fn new(agent: Agent, now: Instant) -> Self {
        Self {
            agent,
            last_visible_activity: now,
            pending_work_start_at: None,
            recent_user_interaction: None,
            status: PaneAgentStatus::Busy,
        }
    }

    fn observe_agent(&mut self, agent: Agent, now: Instant) -> bool {
        if self.agent == agent {
            return false;
        }

        // A different fg agent starts a new lifecycle; old activity, attention, and local-echo suppression
        // belonged to the previous process and must not carry over.
        *self = Self::new(agent, now);
        true
    }

    const fn record_user_interaction(&mut self, interaction: PaneUserInteraction, now: Instant) {
        match interaction {
            PaneUserInteraction::MayEcho => {
                self.recent_user_interaction = Some(now);
            }
            PaneUserInteraction::StartsAgentWork => {
                // Submitting a prompt is user input, but the next redraw is the agent starting work. Clear prior
                // typing suppression so a fast response is not lost as local echo.
                self.pending_work_start_at = Some(now);
                self.recent_user_interaction = None;
            }
        }
    }

    fn record_visible_activity(&mut self, agent: Agent, now: Instant) -> bool {
        if self.agent != agent {
            return false;
        }
        self.discard_stale_user_interaction(now);
        if self.recent_user_interaction.is_some() {
            // User typing and mouse gestures can redraw through the PTY. Those bytes still render, but they are not
            // agent work and must not flip agent attention back to Busy. The suppression is cleared on visible
            // activity too because non-busy agents have no quiet timer to expire it later.
            return false;
        }
        if self.status != PaneAgentStatus::Busy && self.pending_work_start_at.is_none() {
            // Cursor and other terminal agents can repaint idle UI while unfocused. After startup/work has been
            // acknowledged, only a prompt submit is allowed to re-arm agent attention from visible output.
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

    const fn needs_attention(&self) -> bool {
        matches!(self.status, PaneAgentStatus::Unseen)
    }

    const fn state(&self) -> PaneAgentState {
        match self.status {
            PaneAgentStatus::Busy => PaneAgentState::Busy,
            PaneAgentStatus::Seen => PaneAgentState::Seen,
            PaneAgentStatus::Unseen => PaneAgentState::Unseen,
        }
    }

    const fn acknowledge_attention(&mut self) -> bool {
        if !self.needs_attention() {
            return false;
        }
        self.status = PaneAgentStatus::Seen;
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
        if self.status != PaneAgentStatus::Busy {
            return Ok(None);
        }
        self.last_visible_activity
            .checked_add(self::agent_quiet_attention_threshold(self.agent))
            .map(Some)
            .ok_or_else(|| rootcause::report!("muxr pane agent quiet deadline overflowed"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneUserInteraction {
    MayEcho,
    StartsAgentWork,
}

impl PaneAgents {
    pub fn observe_pane_cmd(&mut self, pane_id: PaneId, observation: &PaneCmdObservation, now: Instant) -> bool {
        self.apply_cmd_observation(pane_id, self::agent_observation_from_pane_cmd(observation), now)
    }

    pub fn observe_visible_activity(
        &mut self,
        pane_id: PaneId,
        observation: &PaneCmdObservation,
        now: Instant,
    ) -> bool {
        let mut changed = self.observe_pane_cmd(pane_id, observation, now);
        changed |= self.record_visible_activity(pane_id, self::agent_from_pane_cmd(observation), now);
        changed
    }

    pub fn mark_quiet_deadlines(&mut self, layout: &SessionLayout, now: Instant) -> rootcause::Result<bool> {
        self.retain_layout_panes(layout);
        self.discard_stale_user_interactions(now);
        self.mark_quiet_agents(layout, now)
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
        for pane_agent in self.by_pane.values() {
            let Some(pane_deadline) = pane_agent.quiet_deadline()? else {
                continue;
            };
            deadline = Some(deadline.map_or(pane_deadline, |current: Instant| current.min(pane_deadline)));
        }
        Ok(deadline)
    }

    pub fn snapshot_cmd_labels(&self) -> Vec<(PaneId, Option<String>)> {
        self.by_pane
            .iter()
            .map(|(pane_id, pane_agent)| (*pane_id, Some(pane_agent.agent.short_name().to_owned())))
            .collect()
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

    fn apply_cmd_observation(&mut self, pane_id: PaneId, observation: PaneAgentCmdObservation, now: Instant) -> bool {
        match observation {
            PaneAgentCmdObservation::Agent(agent) => self.observe_agent(pane_id, agent, now),
            PaneAgentCmdObservation::TrustedNoAgent => {
                // A trusted shell/non-agent observation ends the current lifecycle. Unknown observations do not.
                self.by_pane.remove(&pane_id).is_some()
            }
            PaneAgentCmdObservation::Unknown => false,
        }
    }

    fn observe_agent(&mut self, pane_id: PaneId, agent: Agent, now: Instant) -> bool {
        let Some(lifecycle) = self.by_pane.get_mut(&pane_id) else {
            // A newly observed agent starts Busy even if its first dirty frame was missed by the sampled scan.
            // Pre-agent input suppression belongs to the shell and must not hide the new agent's output.
            self.by_pane.insert(pane_id, PaneAgentLifecycle::new(agent, now));
            return true;
        };
        lifecycle.observe_agent(agent, now)
    }

    pub fn record_user_interaction(&mut self, pane_id: PaneId, interaction: PaneUserInteraction, now: Instant) {
        if let Some(lifecycle) = self.by_pane.get_mut(&pane_id) {
            lifecycle.record_user_interaction(interaction, now);
        }
    }

    fn record_visible_activity(&mut self, pane_id: PaneId, agent: Option<Agent>, now: Instant) -> bool {
        let Some(agent) = agent else {
            // PTY output before process detection is still rendered, but it is not agent activity yet. Newly detected
            // agents start Busy instead of inheriting stale shell output from before the scan observed them.
            return false;
        };
        let Some(pane_agent) = self.by_pane.get_mut(&pane_id) else {
            return false;
        };
        pane_agent.record_visible_activity(agent, now)
    }

    fn mark_quiet_if_due(&mut self, pane_id: PaneId, now: Instant, focused: bool) -> bool {
        let Some(pane_agent) = self.by_pane.get_mut(&pane_id) else {
            return false;
        };
        pane_agent.mark_quiet_if_due(now, focused)
    }

    pub fn acknowledge_attention(&mut self, pane_id: PaneId) -> bool {
        self.by_pane
            .get_mut(&pane_id)
            .is_some_and(PaneAgentLifecycle::acknowledge_attention)
    }

    fn needs_attention(&self, pane_id: PaneId) -> bool {
        self.by_pane
            .get(&pane_id)
            .is_some_and(PaneAgentLifecycle::needs_attention)
    }

    pub fn snapshot_states(&self) -> Vec<(PaneId, PaneAgentState)> {
        self.by_pane
            .iter()
            .map(|(pane_id, pane_agent)| (*pane_id, pane_agent.state()))
            .collect()
    }

    fn retain_panes(&mut self, pane_ids: &BTreeSet<PaneId>) {
        self.by_pane.retain(|pane_id, _pane_agent| pane_ids.contains(pane_id));
    }

    fn discard_stale_user_interactions(&mut self, now: Instant) {
        for pane_agent in self.by_pane.values_mut() {
            pane_agent.discard_stale_user_interaction(now);
        }
    }
}

fn layout_pane_ids(layout: &SessionLayout) -> Vec<PaneId> {
    layout.panes().into_iter().map(|pane| pane.id).collect()
}

fn agent_observation_from_pane_cmd(observation: &PaneCmdObservation) -> PaneAgentCmdObservation {
    match observation {
        PaneCmdObservation::FgCmd { .. } => self::agent_from_pane_cmd(observation)
            .map_or(PaneAgentCmdObservation::TrustedNoAgent, PaneAgentCmdObservation::Agent),
        PaneCmdObservation::Shell => PaneAgentCmdObservation::TrustedNoAgent,
        PaneCmdObservation::Unknown { .. } => PaneAgentCmdObservation::Unknown,
    }
}

fn agent_from_pane_cmd(observation: &PaneCmdObservation) -> Option<Agent> {
    let PaneCmdObservation::FgCmd { cmd } = observation else {
        return None;
    };
    crate::pane_cmd::agent_for_cmd(cmd)
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

#[cfg(test)]
mod tests {
    use muxr_core::SessionName;

    use super::*;
    use crate::pane_cmd::PaneCmd;
    use crate::pane_cmd::PaneCmdUnknownReason;
    use crate::pane_split::PaneSplitAxis;
    use crate::state::SessionMetadata;

    fn pane_agent_status(pane_agents: &PaneAgents, pane_id: PaneId) -> PaneAgentState {
        pane_agents
            .by_pane
            .get(&pane_id)
            .map_or(PaneAgentState::NoAgent, PaneAgentLifecycle::state)
    }

    fn set_pane_agent_status(pane_agents: &mut PaneAgents, pane_id: PaneId, status: PaneAgentStatus) {
        if let Some(pane_agent) = pane_agents.by_pane.get_mut(&pane_id) {
            pane_agent.status = status;
        }
    }

    #[test]
    fn test_pane_agent_lifecycle_when_created_starts_busy() {
        let pane_agent = PaneAgentLifecycle::new(Agent::Codex, Instant::now());

        pretty_assertions::assert_eq!(pane_agent.state(), PaneAgentState::Busy);
    }

    #[rstest::rstest]
    #[case::seen(PaneAgentStatus::Seen, PaneAgentState::Seen)]
    #[case::unseen(PaneAgentStatus::Unseen, PaneAgentState::Unseen)]
    fn test_pane_agent_lifecycle_when_user_echoes_visible_activity_does_not_mark_busy(
        #[case] starting_status: PaneAgentStatus,
        #[case] expected_state: PaneAgentState,
    ) -> rootcause::Result<()> {
        let then = Instant::now();
        let mut pane_agent = PaneAgentLifecycle::new(Agent::Codex, then);
        pane_agent.status = starting_status;
        pane_agent.record_user_interaction(PaneUserInteraction::MayEcho, then);

        assert2::assert!(
            !pane_agent.record_visible_activity(Agent::Codex, self::instant_after(then, Duration::from_millis(100))?)
        );

        pretty_assertions::assert_eq!(pane_agent.state(), expected_state);
        Ok(())
    }

    #[test]
    fn test_pane_agent_lifecycle_when_user_echo_suppression_expires_records_busy_activity() -> rootcause::Result<()> {
        let then = Instant::now();
        let mut pane_agent = PaneAgentLifecycle::new(Agent::Codex, then);
        pane_agent.record_user_interaction(PaneUserInteraction::MayEcho, then);
        let visible_activity_at = self::instant_after(then, Duration::from_millis(501))?;

        assert2::assert!(!pane_agent.record_visible_activity(Agent::Codex, visible_activity_at));

        pretty_assertions::assert_eq!(
            pane_agent.quiet_deadline()?,
            Some(self::instant_after(visible_activity_at, Duration::from_secs(3))?)
        );
        Ok(())
    }

    #[test]
    fn test_pane_agent_lifecycle_when_prompt_submit_precedes_visible_activity_marks_busy() -> rootcause::Result<()> {
        let then = Instant::now();
        let mut pane_agent = PaneAgentLifecycle::new(Agent::Codex, then);
        pane_agent.status = PaneAgentStatus::Seen;
        pane_agent.record_user_interaction(PaneUserInteraction::MayEcho, then);
        pane_agent.record_user_interaction(
            PaneUserInteraction::StartsAgentWork,
            self::instant_after(then, Duration::from_millis(100))?,
        );

        assert2::assert!(
            pane_agent.record_visible_activity(Agent::Codex, self::instant_after(then, Duration::from_millis(150))?)
        );

        pretty_assertions::assert_eq!(pane_agent.state(), PaneAgentState::Busy);
        Ok(())
    }

    #[rstest::rstest]
    #[case::focused(true, PaneAgentState::Seen)]
    #[case::unfocused(false, PaneAgentState::Unseen)]
    fn test_pane_agent_lifecycle_when_quiet_deadline_fires_marks_seen_or_unseen(
        #[case] focused: bool,
        #[case] expected_state: PaneAgentState,
    ) -> rootcause::Result<()> {
        let then = Instant::now();
        let mut pane_agent = PaneAgentLifecycle::new(Agent::Codex, then);

        assert2::assert!(pane_agent.mark_quiet_if_due(self::instant_after(then, Duration::from_secs(3))?, focused));

        pretty_assertions::assert_eq!(pane_agent.state(), expected_state);
        Ok(())
    }

    #[test]
    fn test_pane_agent_lifecycle_when_attention_is_acknowledged_marks_seen() {
        let mut pane_agent = PaneAgentLifecycle::new(Agent::Codex, Instant::now());
        pane_agent.status = PaneAgentStatus::Unseen;

        assert2::assert!(pane_agent.acknowledge_attention());

        pretty_assertions::assert_eq!(pane_agent.state(), PaneAgentState::Seen);
    }

    #[test]
    fn test_observe_pane_cmd_when_agent_is_fg_marks_busy() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;

        assert2::assert!(pane_agents.observe_pane_cmd(pane_id, &self::fg_agent(Agent::Codex), Instant::now(),));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        pretty_assertions::assert_eq!(
            pane_agents.snapshot_cmd_labels(),
            vec![(pane_id, Some("cx".to_owned()))]
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::shell(self::shell())]
    #[case::non_agent(self::fg_cmd("nvim"))]
    fn test_observe_pane_cmd_when_trusted_no_agent_clears_state(
        #[case] observation: PaneCmdObservation,
    ) -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        pane_agents.observe_pane_cmd(pane_id, &self::fg_agent(Agent::Codex), then);
        self::set_pane_agent_status(&mut pane_agents, pane_id, PaneAgentStatus::Unseen);

        assert2::assert!(pane_agents.observe_pane_cmd(
            pane_id,
            &observation,
            self::instant_after(then, Duration::from_secs(1))?,
        ));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::NoAgent);
        Ok(())
    }

    #[test]
    fn test_observe_pane_cmd_when_unknown_preserves_state() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        pane_agents.observe_pane_cmd(pane_id, &self::fg_agent(Agent::Codex), then);
        self::set_pane_agent_status(&mut pane_agents, pane_id, PaneAgentStatus::Unseen);

        assert2::assert!(!pane_agents.observe_pane_cmd(
            pane_id,
            &self::unknown(),
            self::instant_after(then, Duration::from_secs(1))?,
        ));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Unseen);
        Ok(())
    }

    #[test]
    fn test_observe_visible_activity_when_unseen_agent_repaints_without_prompt_keeps_attention() -> rootcause::Result<()>
    {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        pane_agents.observe_pane_cmd(pane_id, &self::fg_agent(Agent::Codex), Instant::now());
        self::set_pane_agent_status(&mut pane_agents, pane_id, PaneAgentStatus::Unseen);

        assert2::assert!(
            !pane_agents.observe_visible_activity(pane_id, &self::fg_agent(Agent::Codex), Instant::now(),)
        );

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Unseen);
        Ok(())
    }

    #[test]
    fn test_observe_visible_activity_when_cursor_repaints_after_seen_does_not_mark_busy() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        pane_agents.observe_pane_cmd(pane_id, &self::fg_agent(Agent::Cursor), Instant::now());
        self::set_pane_agent_status(&mut pane_agents, pane_id, PaneAgentStatus::Seen);

        assert2::assert!(!pane_agents.observe_visible_activity(
            pane_id,
            &self::fg_agent(Agent::Cursor),
            Instant::now(),
        ));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_observe_visible_activity_when_user_echoes_output_does_not_mark_busy() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        pane_agents.observe_pane_cmd(pane_id, &self::fg_agent(Agent::Codex), then);
        self::set_pane_agent_status(&mut pane_agents, pane_id, PaneAgentStatus::Seen);
        pane_agents.record_user_interaction(pane_id, PaneUserInteraction::MayEcho, then);

        assert2::assert!(!pane_agents.observe_visible_activity(
            pane_id,
            &self::fg_agent(Agent::Codex),
            self::instant_after(then, Duration::from_millis(100))?,
        ));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_observe_visible_activity_when_prompt_submit_precedes_output_marks_busy() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        pane_agents.observe_pane_cmd(pane_id, &self::fg_agent(Agent::Codex), then);
        self::set_pane_agent_status(&mut pane_agents, pane_id, PaneAgentStatus::Seen);
        pane_agents.record_user_interaction(pane_id, PaneUserInteraction::MayEcho, then);
        pane_agents.record_user_interaction(
            pane_id,
            PaneUserInteraction::StartsAgentWork,
            self::instant_after(then, Duration::from_millis(100))?,
        );

        assert2::assert!(pane_agents.observe_visible_activity(
            pane_id,
            &self::fg_agent(Agent::Codex),
            self::instant_after(then, Duration::from_millis(150))?,
        ));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        Ok(())
    }

    #[test]
    fn test_observe_pane_cmd_when_agent_identity_changes_resets_state() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        let then = Instant::now();
        pane_agents.observe_pane_cmd(pane_id, &self::fg_agent(Agent::Codex), then);
        pane_agents.record_user_interaction(
            pane_id,
            PaneUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_millis(100))?,
        );

        assert2::assert!(pane_agents.observe_pane_cmd(
            pane_id,
            &self::fg_agent(Agent::Cursor),
            self::instant_after(then, Duration::from_millis(150))?,
        ));
        self::set_pane_agent_status(&mut pane_agents, pane_id, PaneAgentStatus::Seen);
        pane_agents.record_user_interaction(
            pane_id,
            PaneUserInteraction::StartsAgentWork,
            self::instant_after(then, Duration::from_millis(175))?,
        );
        assert2::assert!(pane_agents.observe_visible_activity(
            pane_id,
            &self::fg_agent(Agent::Cursor),
            self::instant_after(then, Duration::from_millis(200))?,
        ));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Busy);
        pretty_assertions::assert_eq!(
            pane_agents.snapshot_cmd_labels(),
            vec![(pane_id, Some("cu".to_owned()))]
        );
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
    fn test_mark_quiet_deadlines_when_unfocused_busy_agent_is_quiet_marks_unseen() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut pane_agents = PaneAgents::default();
        let pane_id = PaneId::new(1)?;
        let then = Instant::now();
        pane_agents.observe_pane_cmd(pane_id, &self::fg_agent(Agent::Codex), then);

        pretty_assertions::assert_eq!(
            pane_agents.next_quiet_deadline()?,
            Some(self::instant_after(then, Duration::from_secs(3))?)
        );
        assert2::assert!(
            pane_agents.mark_quiet_deadlines(&layout, self::instant_after(then, Duration::from_secs(3))?,)?
        );

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Unseen);
        pretty_assertions::assert_eq!(pane_agents.attention_pane_ids(&layout), vec![pane_id]);
        pretty_assertions::assert_eq!(pane_agents.next_quiet_deadline()?, None);
        Ok(())
    }

    #[test]
    fn test_acknowledge_agent_attention_when_agent_is_unseen_marks_agent_seen() -> rootcause::Result<()> {
        let mut pane_agents = PaneAgents::default();
        let pane_id = self::pane_id()?;
        pane_agents.observe_pane_cmd(pane_id, &self::fg_agent(Agent::Codex), Instant::now());
        self::set_pane_agent_status(&mut pane_agents, pane_id, PaneAgentStatus::Unseen);

        assert2::assert!(pane_agents.acknowledge_attention(pane_id));

        pretty_assertions::assert_eq!(pane_agent_status(&pane_agents, pane_id), PaneAgentState::Seen);
        Ok(())
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

    fn instant_after(instant: Instant, duration: Duration) -> rootcause::Result<Instant> {
        instant
            .checked_add(duration)
            .ok_or_else(|| rootcause::report!("test instant overflowed"))
    }

    fn fg_agent(agent: Agent) -> PaneCmdObservation {
        let executable = match agent {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Cursor => "cursor-agent",
            Agent::Gemini => "gemini",
            Agent::Opencode => "opencode",
        };
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
