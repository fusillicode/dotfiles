use muxr_core::PaneAgentState;
use ytil_agents::agent::Agent;

use crate::state::Pane;

impl Pane {
    pub fn agent_state_with_cmd_label(&self, cmd_label: Option<&str>) -> PaneAgentState {
        match self.agent_state {
            PaneAgentState::NoAgent if self::detect_agent_from_cmd_label(cmd_label) => PaneAgentState::Busy,
            PaneAgentState::NoAgent => PaneAgentState::NoAgent,
            // Seen is an acknowledged attention state. Shell titles only prove an agent command is still present;
            // a future explicit event source must transition Seen back to Busy for newly-started agent work.
            PaneAgentState::Seen => PaneAgentState::Seen,
            PaneAgentState::Busy => PaneAgentState::Busy,
            PaneAgentState::Unseen => PaneAgentState::Unseen,
        }
    }

    pub const fn acknowledge_agent_attention(&mut self) -> bool {
        if !self.agent_state.needs_attention() {
            return false;
        }
        self.agent_state = PaneAgentState::Seen;
        true
    }
}

fn detect_agent_from_cmd_label(cmd_label: Option<&str>) -> bool {
    cmd_label.is_some_and(|cmd_label| Agent::detect(cmd_label).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::PaneAttentionState;
    use crate::state::PaneState;

    #[test]
    fn test_agent_state_with_cmd_label_when_cmd_label_is_agent_returns_busy() -> rootcause::Result<()> {
        let pane = self::pane()?;

        pretty_assertions::assert_eq!(pane.agent_state_with_cmd_label(Some("codex")), PaneAgentState::Busy);
        Ok(())
    }

    #[test]
    fn test_agent_state_with_cmd_label_when_pane_needs_attention_returns_unseen() -> rootcause::Result<()> {
        let mut pane = self::pane()?;
        pane.agent_state = PaneAgentState::Unseen;

        pretty_assertions::assert_eq!(pane.agent_state_with_cmd_label(Some("codex")), PaneAgentState::Unseen);
        Ok(())
    }

    #[test]
    fn test_agent_state_with_cmd_label_when_seen_keeps_seen_until_explicit_transition() -> rootcause::Result<()> {
        let mut pane = self::pane()?;
        pane.agent_state = PaneAgentState::Seen;

        pretty_assertions::assert_eq!(pane.agent_state_with_cmd_label(Some("codex")), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_acknowledge_agent_attention_when_agent_is_unseen_marks_agent_seen() -> rootcause::Result<()> {
        let mut pane = self::pane()?;
        pane.agent_state = PaneAgentState::Unseen;

        assert2::assert!(pane.acknowledge_agent_attention());

        pretty_assertions::assert_eq!(pane.agent_state, PaneAgentState::Seen);
        Ok(())
    }

    fn pane() -> rootcause::Result<Pane> {
        Ok(Pane {
            agent_state: PaneAgentState::NoAgent,
            attention_state: PaneAttentionState::Idle,
            cmd_label: "sh".to_owned(),
            cwd: "/tmp".to_owned(),
            focus_seq: 1,
            id: muxr_core::PaneId::new("pane-1")?,
            started_at: 1,
            state: PaneState::Running,
            title: "sh".to_owned(),
        })
    }
}
