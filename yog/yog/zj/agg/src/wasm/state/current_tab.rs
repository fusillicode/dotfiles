use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use agg::AgentState;
use agg::Cmd;
use agg::GitStat;
use agg::TabIndicator;
use ytil_agents::agent::Agent;

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Default)]
pub struct CurrentTab {
    pub tab_id: usize,
    pub seq: u64,
    pub pane_ids: HashSet<u32>,
    pub missed_pane_updates_by_pane: HashMap<u32, u8>,
    pub pending_activation_focus_ack: bool,
    pub focused_pane: Option<FocusedPane>,
    pub active_focus_pane_id: Option<u32>,
    pub last_focused_agent_pane_id: Option<u32>,
    pub pane_state_by_pane: HashMap<u32, AgentPaneState>,
    pub cwd: Option<PathBuf>,
    pub git_stat: GitStat,
}

impl CurrentTab {
    pub fn new(tab_id: usize) -> Self {
        Self {
            tab_id,
            ..Default::default()
        }
    }

    pub fn tab_indicator(&self) -> TabIndicator {
        if self.pane_state_by_pane.is_empty() {
            TabIndicator::None
        } else if self
            .pane_state_by_pane
            .values()
            .any(|pane_state| pane_state.phase == AgentPanePhase::AttentionUnseen)
        {
            TabIndicator::Red
        } else if self
            .pane_state_by_pane
            .values()
            .any(|pane_state| pane_state.phase == AgentPanePhase::Running)
        {
            TabIndicator::Green
        } else {
            TabIndicator::Empty
        }
    }

    pub fn display_cmd(&self) -> Cmd {
        if let Some(pane_id) = self.winner_pane_id()
            && let Some(pane_state) = self.pane_state_by_pane.get(&pane_id)
        {
            return pane_state.cmd();
        }

        self.focused_running_cmd()
    }

    pub fn current_row_display(&self, is_active: bool) -> (Cmd, TabIndicator) {
        if !self.is_active_mat(is_active) {
            return (self.display_cmd(), self.tab_indicator());
        }

        if let Some(pane_id) = self.first_unfocused_pane_in_phase(AgentPanePhase::AttentionUnseen)
            && let Some(pane_state) = self.pane_state_by_pane.get(&pane_id)
        {
            let cmd = pane_state.cmd();
            let indicator = TabIndicator::from_cmd(&cmd);
            return (cmd, indicator);
        }

        let focused_pane_id = self
            .focused_pane
            .as_ref()
            .map(|focused_pane| focused_pane.id)
            .or(self.active_focus_pane_id);
        if let Some(pane_id) = focused_pane_id
            && let Some(pane_state) = self.pane_state_by_pane.get(&pane_id)
        {
            let cmd = pane_state.cmd();
            let indicator = TabIndicator::from_cmd(&cmd);
            return (cmd, indicator);
        }

        if focused_pane_id.is_some() || self.focused_pane.is_some() {
            return (self.focused_running_cmd(), TabIndicator::None);
        }

        (self.display_cmd(), self.tab_indicator())
    }

    fn focused_running_cmd(&self) -> Cmd {
        self.focused_pane
            .as_ref()
            .and_then(|focused_pane| match focused_pane.label.as_ref() {
                Some(FocusedPaneLabel::TerminalCommand(command) | FocusedPaneLabel::Title(command)) => {
                    Some(Cmd::Running(command.clone()))
                }
                None => None,
            })
            .unwrap_or(Cmd::None)
    }

    fn is_active_mat(&self, is_active: bool) -> bool {
        is_active && self.pane_ids.len() > 1
    }

    fn winner_pane_id(&self) -> Option<u32> {
        self.first_pane_in_phase(AgentPanePhase::AttentionUnseen)
            .or_else(|| self.first_pane_in_phase(AgentPanePhase::Running))
            .or_else(|| {
                self.last_focused_agent_pane_id
                    .filter(|pane_id| self.pane_state_by_pane.contains_key(pane_id))
            })
            .or_else(|| self.pane_state_by_pane.keys().min().copied())
    }

    fn first_pane_in_phase(&self, phase: AgentPanePhase) -> Option<u32> {
        self.pane_state_by_pane
            .iter()
            .filter(|(_, pane_state)| pane_state.phase == phase)
            .min_by_key(|(_, pane_state)| pane_state.phase_seq)
            .map(|(&pane_id, _)| pane_id)
    }

    fn first_unfocused_pane_in_phase(&self, phase: AgentPanePhase) -> Option<u32> {
        self.pane_state_by_pane
            .iter()
            .filter(|(_, pane_state)| pane_state.phase == phase && pane_state.focus == PaneFocus::Unfocused)
            .min_by_key(|(_, pane_state)| pane_state.phase_seq)
            .map(|(&pane_id, _)| pane_id)
    }

    pub fn sync_active_focus(&mut self, new_pane_id: Option<u32>, acknowledge_existing_attention: bool) {
        self.active_focus_pane_id = new_pane_id;
        for pane_state in self.pane_state_by_pane.values_mut() {
            pane_state.focus = PaneFocus::Unfocused;
        }

        let Some(pane_id) = new_pane_id else {
            return;
        };
        let Some(pane_state) = self.pane_state_by_pane.get_mut(&pane_id) else {
            return;
        };

        pane_state.focus = PaneFocus::Focused;
        if acknowledge_existing_attention && pane_state.phase == AgentPanePhase::AttentionUnseen {
            pane_state.phase = AgentPanePhase::AttentionSeen;
        }
        self.last_focused_agent_pane_id = Some(pane_id);
    }

    pub fn clear_active_focus(&mut self) {
        self.active_focus_pane_id = None;
        self.pending_activation_focus_ack = false;
        for pane_state in self.pane_state_by_pane.values_mut() {
            pane_state.focus = PaneFocus::Unfocused;
        }
    }

    pub fn transition_phase(&mut self, pane_id: u32, agent: Agent, phase: AgentPanePhase) {
        let focus = if self.active_focus_pane_id == Some(pane_id) {
            PaneFocus::Focused
        } else {
            PaneFocus::Unfocused
        };
        self.pane_state_by_pane.insert(
            pane_id,
            AgentPaneState {
                agent,
                phase,
                focus,
                phase_seq: self.seq.saturating_add(1),
            },
        );
        if focus == PaneFocus::Focused {
            self.last_focused_agent_pane_id = Some(pane_id);
        }
    }
}

#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentPaneState {
    pub agent: Agent,
    pub phase: AgentPanePhase,
    pub focus: PaneFocus,
    pub phase_seq: u64,
}

impl AgentPaneState {
    const fn cmd(self) -> Cmd {
        Cmd::agent(
            self.agent,
            match self.phase {
                AgentPanePhase::Running => AgentState::Busy,
                AgentPanePhase::AttentionUnseen => AgentState::NeedsAttention,
                AgentPanePhase::AttentionSeen => AgentState::Acknowledged,
            },
        )
    }
}

#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum AgentPanePhase {
    Running,
    AttentionUnseen,
    AttentionSeen,
}

#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum PaneFocus {
    Focused,
    Unfocused,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FocusedPane {
    pub id: u32,
    pub label: Option<FocusedPaneLabel>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FocusedPaneLabel {
    TerminalCommand(String),
    Title(String),
}

pub fn idle_phase_for_pane(current_tab: &CurrentTab, current_tab_is_active: bool, pane_id: u32) -> AgentPanePhase {
    if current_tab_is_active && current_tab.active_focus_pane_id == Some(pane_id) {
        AgentPanePhase::AttentionSeen
    } else {
        AgentPanePhase::AttentionUnseen
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::wasm::state::test_support::*;

    #[test]
    fn test_mat_indicator_red_wins_over_green() {
        let current_tab = CurrentTab {
            pane_ids: [42, 43].into_iter().collect(),
            pane_state_by_pane: HashMap::from([
                (
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                ),
                (
                    43,
                    pane_state(Agent::Claude, AgentPanePhase::Running, PaneFocus::Unfocused, 2),
                ),
            ]),
            ..CurrentTab::new(10)
        };

        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::NeedsAttention)
        );
    }

    #[test]
    fn test_current_row_display_inactive_mat_focused_running_agent_does_not_hide_red() {
        let current_tab = CurrentTab {
            pane_ids: [42, 43].into_iter().collect(),
            focused_pane: Some(FocusedPane {
                id: 43,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
            active_focus_pane_id: Some(43),
            pane_state_by_pane: HashMap::from([
                (
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                ),
                (
                    43,
                    pane_state(Agent::Claude, AgentPanePhase::Running, PaneFocus::Focused, 2),
                ),
            ]),
            ..CurrentTab::new(10)
        };

        assert_eq!(
            current_tab.current_row_display(false),
            (Cmd::agent(Agent::Codex, AgentState::NeedsAttention), TabIndicator::Red,)
        );
    }

    #[test]
    fn test_current_row_display_active_mat_hides_dot_for_focused_non_agent_until_other_pane_turns_red() {
        let mut current_tab = CurrentTab {
            pane_ids: [42, 43].into_iter().collect(),
            focused_pane: Some(FocusedPane {
                id: 43,
                label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
            }),
            active_focus_pane_id: Some(43),
            pane_state_by_pane: HashMap::from([(
                42,
                pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
            )]),
            ..CurrentTab::new(10)
        };

        assert_eq!(
            current_tab.current_row_display(true),
            (Cmd::Running("cargo".to_string()), TabIndicator::None)
        );

        current_tab.transition_phase(42, Agent::Codex, AgentPanePhase::AttentionUnseen);
        assert_eq!(
            current_tab.current_row_display(true),
            (Cmd::agent(Agent::Codex, AgentState::NeedsAttention), TabIndicator::Red,)
        );
    }

    #[test]
    fn test_current_row_display_active_mat_hides_dot_for_blank_focused_non_agent_pane() {
        let current_tab = CurrentTab {
            pane_ids: [42, 43].into_iter().collect(),
            focused_pane: Some(FocusedPane { id: 43, label: None }),
            active_focus_pane_id: Some(43),
            pane_state_by_pane: HashMap::from([(
                42,
                pane_state(Agent::Claude, AgentPanePhase::AttentionSeen, PaneFocus::Unfocused, 1),
            )]),
            ..CurrentTab::new(10)
        };

        assert_eq!(current_tab.current_row_display(true), (Cmd::None, TabIndicator::None));
    }

    #[test]
    fn test_current_row_display_inactive_mat_uses_aggregate_priority() {
        let current_tab = CurrentTab {
            pane_ids: [42, 43].into_iter().collect(),
            focused_pane: Some(FocusedPane {
                id: 43,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
            active_focus_pane_id: Some(43),
            pane_state_by_pane: HashMap::from([
                (
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
                ),
                (
                    43,
                    pane_state(Agent::Claude, AgentPanePhase::Running, PaneFocus::Focused, 2),
                ),
            ]),
            ..CurrentTab::new(10)
        };

        assert_eq!(
            current_tab.current_row_display(false),
            (Cmd::agent(Agent::Codex, AgentState::Busy), TabIndicator::Green,)
        );
    }

    #[test]
    fn test_current_row_display_inactive_mat_blank_focused_non_agent_uses_red_over_green() {
        let current_tab = CurrentTab {
            pane_ids: [42, 43, 44].into_iter().collect(),
            focused_pane: Some(FocusedPane { id: 43, label: None }),
            active_focus_pane_id: Some(43),
            pane_state_by_pane: HashMap::from([
                (
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                ),
                (
                    44,
                    pane_state(Agent::Claude, AgentPanePhase::Running, PaneFocus::Unfocused, 2),
                ),
            ]),
            ..CurrentTab::new(10)
        };

        assert_eq!(
            current_tab.current_row_display(false),
            (Cmd::agent(Agent::Codex, AgentState::NeedsAttention), TabIndicator::Red,)
        );
    }

    #[test]
    fn test_current_row_display_inactive_mat_blank_focused_non_agent_uses_green_over_empty() {
        let current_tab = CurrentTab {
            pane_ids: [42, 43, 44].into_iter().collect(),
            focused_pane: Some(FocusedPane { id: 43, label: None }),
            active_focus_pane_id: Some(43),
            pane_state_by_pane: HashMap::from([
                (
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
                ),
                (
                    44,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionSeen, PaneFocus::Unfocused, 2),
                ),
            ]),
            ..CurrentTab::new(10)
        };

        assert_eq!(
            current_tab.current_row_display(false),
            (Cmd::agent(Agent::Codex, AgentState::Busy), TabIndicator::Green,)
        );
    }

    #[test]
    fn test_new_state_starts_without_indicator_after_restart() {
        let current_tab = CurrentTab::new(10);

        assert_eq!(current_tab.tab_indicator(), TabIndicator::None);
        assert_eq!(current_tab.display_cmd(), Cmd::None);
    }
}
