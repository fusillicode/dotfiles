use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use agg::AgentState;
use agg::Cmd;
use agg::GitStat;
use agg::TabIndicator;
use ytil_agents::agent::Agent;

use crate::plugin::pane::FocusedPane;
use crate::plugin::pane::FocusedPaneLabel;

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
    pub cwd_pane_id: Option<u32>,
    pub cwd: Option<PathBuf>,
    pub git_stat: GitStat,
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct CurrentTabDisplay {
    pub pane_id: Option<u32>,
    pub cwd: Option<PathBuf>,
    pub cmd: Cmd,
    pub indicator: TabIndicator,
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
            TabIndicator::NoAgent
        } else if self
            .pane_state_by_pane
            .values()
            .any(|pane_state| pane_state.phase == AgentPanePhase::AttentionUnseen)
        {
            TabIndicator::Unseen
        } else if self
            .pane_state_by_pane
            .values()
            .any(|pane_state| pane_state.phase == AgentPanePhase::Running)
        {
            TabIndicator::Busy
        } else {
            TabIndicator::Seen
        }
    }

    pub fn display_cmd(&self) -> Cmd {
        self.display_cmd_source().1
    }

    fn display_cmd_source(&self) -> (Option<u32>, Cmd) {
        if let Some(pane_id) = self.winner_pane_id()
            && let Some(pane_state) = self.pane_state_by_pane.get(&pane_id)
        {
            return (Some(pane_id), pane_state.cmd());
        }

        (
            self.focused_pane.as_ref().map(|focused_pane| focused_pane.id),
            self.focused_running_cmd(),
        )
    }

    pub fn current_row_display(&self, is_active: bool) -> (Cmd, TabIndicator) {
        let (_, cmd, indicator) = self.current_row_display_parts(is_active);
        (cmd, indicator)
    }

    pub fn current_row_display_source(
        &self,
        is_active: bool,
        cwds_by_pane: &HashMap<u32, PathBuf>,
    ) -> CurrentTabDisplay {
        let (pane_id, cmd, indicator) = self.current_row_display_parts(is_active);
        let cwd = pane_id.and_then(|pane_id| self.cwd_for_pane(pane_id, cwds_by_pane));
        let git_stat = self.git_stat_for_display(cwd.as_ref());
        CurrentTabDisplay {
            pane_id,
            cwd,
            cmd,
            indicator,
            git_stat,
        }
    }

    fn git_stat_for_display(&self, cwd: Option<&PathBuf>) -> GitStat {
        // Keep all visible row data tied to the selected pane target; stale git state from another cwd must not
        // survive when an unfocused agent pane wins the tab display.
        if cwd == Some(&self.git_stat.path) {
            self.git_stat.clone()
        } else {
            GitStat::default()
        }
    }

    fn current_row_display_parts(&self, is_active: bool) -> (Option<u32>, Cmd, TabIndicator) {
        if !self.is_active_mat(is_active) {
            let (pane_id, cmd) = self.display_cmd_source();
            return (pane_id, cmd, self.tab_indicator());
        }

        if let Some(pane_id) = self.first_unfocused_pane_in_phase(AgentPanePhase::AttentionUnseen)
            && let Some(pane_state) = self.pane_state_by_pane.get(&pane_id)
        {
            let cmd = pane_state.cmd();
            let indicator = TabIndicator::from_cmd(&cmd);
            return (Some(pane_id), cmd, indicator);
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
            return (Some(pane_id), cmd, indicator);
        }

        if focused_pane_id.is_some() || self.focused_pane.is_some() {
            return (focused_pane_id, self.focused_running_cmd(), TabIndicator::NoAgent);
        }

        let (pane_id, cmd) = self.display_cmd_source();
        (pane_id, cmd, self.tab_indicator())
    }

    fn cwd_for_pane(&self, pane_id: u32, cwds_by_pane: &HashMap<u32, PathBuf>) -> Option<PathBuf> {
        cwds_by_pane
            .get(&pane_id)
            .cloned()
            .or_else(|| (self.cwd_pane_id == Some(pane_id)).then(|| self.cwd.clone()).flatten())
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

    pub fn transition_phase(&mut self, pane_id: u32, agent: Agent, phase: AgentPanePhase, source: AgentPaneSource) {
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
                source,
                phase_seq: self.seq.saturating_add(1),
            },
        );
        if focus == PaneFocus::Focused {
            self.last_focused_agent_pane_id = Some(pane_id);
        }
    }

    pub fn apply_panes_changed(&mut self, observed_pane_ids: &HashSet<u32>, retained_pane_ids: &HashSet<u32>) {
        self.pane_ids.clone_from(retained_pane_ids);
        self.missed_pane_updates_by_pane
            .retain(|pane_id, _| retained_pane_ids.contains(pane_id));
        for pane_id in retained_pane_ids {
            if observed_pane_ids.contains(pane_id) {
                self.missed_pane_updates_by_pane.remove(pane_id);
            } else {
                let missed = self.missed_pane_updates_by_pane.entry(*pane_id).or_insert(0);
                *missed = missed.saturating_add(1);
            }
        }
        self.pane_state_by_pane
            .retain(|pane_id, _| retained_pane_ids.contains(pane_id));
        if self
            .last_focused_agent_pane_id
            .is_some_and(|pane_id| !retained_pane_ids.contains(&pane_id))
        {
            self.last_focused_agent_pane_id = None;
        }
        if self
            .active_focus_pane_id
            .is_some_and(|pane_id| !retained_pane_ids.contains(&pane_id))
        {
            self.clear_active_focus();
        }
        self.seq = self.seq.saturating_add(1);
    }

    pub fn apply_agent_lost(&mut self, pane_id: u32, source: AgentLossSource) {
        // Wrapper-launched agents can leave Zellij's manifest showing the wrapper/title.
        // Preserve pipe-owned agent state until the pipe exit or pane close owns the loss.
        if source == AgentLossSource::Manifest
            && self
                .pane_state_by_pane
                .get(&pane_id)
                .is_some_and(|pane_state| pane_state.source == AgentPaneSource::Pipe)
        {
            return;
        }
        self.pane_state_by_pane.remove(&pane_id);
        self.missed_pane_updates_by_pane.remove(&pane_id);
        if self.last_focused_agent_pane_id == Some(pane_id) {
            self.last_focused_agent_pane_id = None;
        }
        if self.active_focus_pane_id == Some(pane_id) {
            self.active_focus_pane_id = None;
        }
        self.seq = self.seq.saturating_add(1);
    }
}

#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentPaneState {
    pub agent: Agent,
    pub phase: AgentPanePhase,
    pub focus: PaneFocus,
    pub source: AgentPaneSource,
    pub phase_seq: u64,
}

impl AgentPaneState {
    pub const fn cmd(self) -> Cmd {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentPaneSource {
    Manifest,
    Pipe,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentLossSource {
    Manifest,
    Pipe,
    PaneClosed,
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

    use crate::plugin::pane::FocusedPane;
    use crate::plugin::pane::FocusedPaneLabel;
    use crate::plugin::tbar::current_tab::*;
    use crate::plugin::tbar::test_support::*;

    #[test]
    fn test_mat_indicator_unseen_wins_over_busy() {
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

        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Unseen);
        pretty_assertions::assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::NeedsAttention)
        );
    }

    #[test]
    fn test_current_row_display_inactive_mat_focused_running_agent_does_not_hide_unseen() {
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

        pretty_assertions::assert_eq!(
            current_tab.current_row_display(false),
            (
                Cmd::agent(Agent::Codex, AgentState::NeedsAttention),
                TabIndicator::Unseen,
            )
        );
    }

    #[test]
    fn test_current_row_display_active_mat_hides_dot_for_focused_non_agent_until_other_pane_unseen() {
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

        pretty_assertions::assert_eq!(
            current_tab.current_row_display(true),
            (Cmd::Running("cargo".to_string()), TabIndicator::NoAgent)
        );

        current_tab.transition_phase(
            42,
            Agent::Codex,
            AgentPanePhase::AttentionUnseen,
            AgentPaneSource::Manifest,
        );
        pretty_assertions::assert_eq!(
            current_tab.current_row_display(true),
            (
                Cmd::agent(Agent::Codex, AgentState::NeedsAttention),
                TabIndicator::Unseen,
            )
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

        pretty_assertions::assert_eq!(
            current_tab.current_row_display(true),
            (Cmd::None, TabIndicator::NoAgent)
        );
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

        pretty_assertions::assert_eq!(
            current_tab.current_row_display(false),
            (Cmd::agent(Agent::Codex, AgentState::Busy), TabIndicator::Busy,)
        );
    }

    #[test]
    fn test_current_row_display_source_inactive_uses_winner_pane_cwd() {
        let current_tab = CurrentTab {
            pane_ids: [42, 43].into_iter().collect(),
            focused_pane: Some(FocusedPane {
                id: 43,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
            active_focus_pane_id: Some(43),
            cwd_pane_id: Some(43),
            cwd: Some(PathBuf::from("/Users/me/claude")),
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
        let cwds_by_pane = HashMap::from([
            (42, PathBuf::from("/Users/me/codex")),
            (43, PathBuf::from("/Users/me/claude")),
        ]);

        pretty_assertions::assert_eq!(
            current_tab.current_row_display_source(false, &cwds_by_pane),
            CurrentTabDisplay {
                pane_id: Some(42),
                cwd: Some(PathBuf::from("/Users/me/codex")),
                cmd: Cmd::agent(Agent::Codex, AgentState::Busy),
                indicator: TabIndicator::Busy,
                git_stat: GitStat::default(),
            }
        );
    }

    #[test]
    fn test_current_row_display_source_uses_git_stat_for_display_cwd_only() {
        let current_tab = CurrentTab {
            pane_ids: [42, 43].into_iter().collect(),
            git_stat: GitStat {
                path: PathBuf::from("/Users/me/shell"),
                insertions: 7,
                ..GitStat::default()
            },
            pane_state_by_pane: HashMap::from([(
                42,
                pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
            )]),
            ..CurrentTab::new(10)
        };
        let cwds_by_pane = HashMap::from([
            (42, PathBuf::from("/Users/me/codex")),
            (43, PathBuf::from("/Users/me/shell")),
        ]);

        let display = current_tab.current_row_display_source(false, &cwds_by_pane);

        pretty_assertions::assert_eq!(display.pane_id, Some(42));
        pretty_assertions::assert_eq!(display.cwd, Some(PathBuf::from("/Users/me/codex")));
        pretty_assertions::assert_eq!(display.git_stat, GitStat::default());
    }

    #[test]
    fn test_current_row_display_source_reuses_git_stat_for_same_display_cwd() {
        let display_cwd = PathBuf::from("/Users/me/project");
        let git_stat = GitStat {
            path: display_cwd.clone(),
            insertions: 7,
            ..GitStat::default()
        };
        let current_tab = CurrentTab {
            pane_ids: [42, 43].into_iter().collect(),
            git_stat: git_stat.clone(),
            pane_state_by_pane: HashMap::from([(
                42,
                pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
            )]),
            ..CurrentTab::new(10)
        };
        let cwds_by_pane = HashMap::from([(42, display_cwd.clone()), (43, display_cwd)]);

        let display = current_tab.current_row_display_source(false, &cwds_by_pane);

        pretty_assertions::assert_eq!(display.pane_id, Some(42));
        pretty_assertions::assert_eq!(display.git_stat, git_stat);
    }

    #[test]
    fn test_current_row_display_source_uses_previous_same_pane_cwd_when_cache_misses() {
        let current_tab = CurrentTab {
            pane_ids: [42, 43].into_iter().collect(),
            cwd_pane_id: Some(42),
            cwd: Some(PathBuf::from("/Users/me/codex")),
            pane_state_by_pane: HashMap::from([(
                42,
                pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
            )]),
            ..CurrentTab::new(10)
        };
        let cwds_by_pane = HashMap::from([(43, PathBuf::from("/Users/me/other"))]);

        pretty_assertions::assert_eq!(
            current_tab.current_row_display_source(false, &cwds_by_pane).cwd,
            Some(PathBuf::from("/Users/me/codex"))
        );
    }

    #[test]
    fn test_current_row_display_inactive_mat_blank_focused_non_agent_uses_unseen_over_busy() {
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

        pretty_assertions::assert_eq!(
            current_tab.current_row_display(false),
            (
                Cmd::agent(Agent::Codex, AgentState::NeedsAttention),
                TabIndicator::Unseen,
            )
        );
    }

    #[test]
    fn test_current_row_display_inactive_mat_blank_focused_non_agent_uses_busy_over_seen() {
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

        pretty_assertions::assert_eq!(
            current_tab.current_row_display(false),
            (Cmd::agent(Agent::Codex, AgentState::Busy), TabIndicator::Busy,)
        );
    }

    #[test]
    fn test_new_state_starts_without_indicator_after_restart() {
        let current_tab = CurrentTab::new(10);

        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::NoAgent);
        pretty_assertions::assert_eq!(current_tab.display_cmd(), Cmd::None);
    }
}
