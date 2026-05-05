use std::path::Path;

use ytil_agents::agent::Agent;
use zellij_tile::prelude::TabInfo;

use super::State;
use super::current_tab::AgentPanePhase;
use super::current_tab::CurrentTab;

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct Nudge {
    pub agent: Agent,
    pub tab_id: usize,
    pub pane_id: u32,
    pub path: String,
}

impl Nudge {
    pub fn new(current_tab: &CurrentTab, tabs: &[TabInfo], home_dir: &Path, pane_id: u32) -> Option<Self> {
        let pane_state = current_tab.pane_state_by_pane.get(&pane_id)?;
        if pane_state.phase != AgentPanePhase::AttentionUnseen {
            return None;
        }

        let path = current_tab.cwd.as_ref().map_or_else(
            || {
                tabs.iter()
                    .find(|tab| tab.tab_id == current_tab.tab_id)
                    .map_or_else(String::new, |tab| tab.name.clone())
            },
            |path| ytil_tui::short_path(path, home_dir),
        );

        Some(Self {
            agent: pane_state.agent,
            tab_id: current_tab.tab_id,
            pane_id,
            path,
        })
    }

    pub fn title(&self) -> String {
        format!("🔔 {} done", self.agent)
    }

    pub fn body(&self) -> String {
        let location = format!("t{} p{}", self.tab_id, self.pane_id);
        if self.path.is_empty() {
            return location;
        }
        format!("{} · {}", self.path, location)
    }
}

impl State {
    pub fn nudges(&self) -> Vec<(u32, Nudge)> {
        let Some(current_tab) = self.current_tab.as_ref() else {
            return vec![];
        };
        let mut nudges = current_tab
            .pane_state_by_pane
            .iter()
            .filter(|(pane_id, _)| !self.nudged_pane_ids.contains(*pane_id))
            .filter_map(|(pane_id, pane_state)| {
                Nudge::new(current_tab, &self.all_tabs, &self.home_dir, *pane_id)
                    .map(|nudge| (pane_state.phase_seq, *pane_id, nudge))
            })
            .collect::<Vec<_>>();
        nudges.sort_by_key(|(phase_seq, pane_id, _)| (*phase_seq, *pane_id));
        nudges.into_iter().map(|(_, pane_id, nudge)| (pane_id, nudge)).collect()
    }

    pub fn mark_nudged(&mut self, pane_id: u32) {
        self.nudged_pane_ids.insert(pane_id);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::path::PathBuf;

    use assert2::assert;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::wasm::state::current_tab::FocusedPane;
    use crate::wasm::state::current_tab::FocusedPaneLabel;
    use crate::wasm::state::current_tab::PaneFocus;
    use crate::wasm::state::test_support::*;

    #[test]
    fn test_nudges_include_attention_from_any_current_tab_focus_state() {
        let state = State {
            known_active_tab_id: Some(10),
            current_tab: Some(CurrentTab {
                pane_ids: HashSet::from([42, 99]),
                focused_pane: Some(FocusedPane {
                    id: 99,
                    label: Some(FocusedPaneLabel::TerminalCommand(String::new())),
                }),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::AttentionUnseen, PaneFocus::Focused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            home_dir: PathBuf::from("/Users/me"),
            all_tabs: vec![tab_with_name(10, 0, "project")],
            ..Default::default()
        };

        let nudges = state.nudges();
        assert!(let [(42, nudge)] = nudges.as_slice());
        assert_eq!(nudge.agent, Agent::Codex);
        assert_eq!(nudge.tab_id, 10);
        assert_eq!(nudge.pane_id, 42);
        assert_eq!(nudge.path, "project");
        assert_eq!(nudge.title(), "🔔 Codex done");
        assert_eq!(nudge.body(), "project · t10 p42");
    }
}
