use std::collections::HashSet;

use zellij_tile::prelude::TabInfo;

use super::State;
use super::current_tab::AgentPanePhase;
use super::current_tab::CurrentTab;
use super::current_tab::FocusedPane;
use super::current_tab::idle_phase_for_pane;
use crate::wasm::events::StateEvent;
use crate::wasm::plugin::StateSnapshotPayload;

impl State {
    pub fn apply_all(&mut self, events: &[StateEvent]) -> bool {
        for event in events {
            self.apply(event);
        }
        self.prune_nudges();
        self.sync_frame()
    }

    fn apply(&mut self, event: &StateEvent) {
        match event {
            StateEvent::TabCreated { tab_id } => {
                self.current_tab = Some(CurrentTab::new(*tab_id));
            }
            StateEvent::TabRemapped { new_tab_id } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.tab_id = *new_tab_id;
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            StateEvent::PanesChanged {
                observed_pane_ids,
                retained_pane_ids,
            } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.apply_panes_changed(observed_pane_ids, retained_pane_ids);
                }
            }
            StateEvent::FocusChanged {
                new_pane,
                acknowledge_existing_attention,
            } => self.apply_focus_changed(new_pane.as_ref(), *acknowledge_existing_attention),
            StateEvent::CwdChanged { new_cwd } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.cwd = Some(new_cwd.clone());
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            StateEvent::AgentDetected { pane_id, agent } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.transition_phase(*pane_id, *agent, AgentPanePhase::AttentionSeen);
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            StateEvent::AgentBusy { pane_id, agent } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.transition_phase(*pane_id, *agent, AgentPanePhase::Running);
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            StateEvent::AgentIdle { pane_id, agent } => {
                let current_tab_is_active = self.current_tab_is_active();
                if let Some(current_tab) = self.current_tab.as_mut() {
                    let phase = idle_phase_for_pane(current_tab, current_tab_is_active, *pane_id);
                    current_tab.transition_phase(*pane_id, *agent, phase);
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            StateEvent::AgentLost { pane_id } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.apply_agent_lost(*pane_id);
                }
            }
            StateEvent::GitStatChanged { new_stat } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.git_stat = *new_stat;
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            StateEvent::RemoteTabUpdated {
                source_plugin_id,
                snapshot,
                evict_ids,
            } => self.apply_remote_tab_updated(*source_plugin_id, snapshot, evict_ids),
            StateEvent::ActiveTabChanged { active_tab_id } => self.apply_active_tab_changed(*active_tab_id),
            StateEvent::AllTabsReplaced { new_tabs } => self.apply_all_tabs_replaced(new_tabs),
            StateEvent::SyncRequested => {
                self.sync_requested = true;
            }
            StateEvent::BecameActive => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.pending_activation_focus_ack = true;
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            StateEvent::TopologyChanged => {}
        }
    }

    fn apply_focus_changed(&mut self, new_pane: Option<&FocusedPane>, acknowledge_existing_attention: bool) {
        let is_active = self.current_tab_is_active();
        let Some(current_tab) = self.current_tab.as_mut() else {
            return;
        };
        current_tab.focused_pane = new_pane.cloned();
        if is_active {
            current_tab.sync_active_focus(
                new_pane.map(|focused_pane| focused_pane.id),
                acknowledge_existing_attention,
            );
            if acknowledge_existing_attention {
                current_tab.pending_activation_focus_ack = false;
            }
        } else {
            current_tab.clear_active_focus();
        }
        current_tab.seq = current_tab.seq.saturating_add(1);
    }

    fn apply_remote_tab_updated(&mut self, source_plugin_id: u32, snapshot: &StateSnapshotPayload, evict_ids: &[u32]) {
        for evict_id in evict_ids {
            self.other_tabs.remove(evict_id);
        }
        self.other_tabs.insert(source_plugin_id, snapshot.clone());
    }

    fn apply_active_tab_changed(&mut self, active_tab_id: usize) {
        let was_active = self.current_tab_is_active();
        self.known_active_tab_id = Some(active_tab_id);
        self.sync_active_change(was_active);
    }

    fn apply_all_tabs_replaced(&mut self, new_tabs: &[TabInfo]) {
        let was_active = self.current_tab_is_active();
        let known_tab_ids: HashSet<usize> = new_tabs.iter().map(|tab| tab.tab_id).collect();
        self.other_tabs
            .retain(|_, remote| known_tab_ids.contains(&remote.tab_id));
        self.known_active_tab_id = new_tabs.iter().find(|tab| tab.active).map(|tab| tab.tab_id);
        self.all_tabs.clone_from(&new_tabs.to_vec());
        self.sync_active_change(was_active);
    }

    fn prune_nudges(&mut self) {
        let Some(current_tab) = self.current_tab.as_ref() else {
            self.nudged_pane_ids.clear();
            return;
        };
        self.nudged_pane_ids.retain(|pane_id| {
            current_tab
                .pane_state_by_pane
                .get(pane_id)
                .is_some_and(|pane_state| pane_state.phase == AgentPanePhase::AttentionUnseen)
        });
    }

    fn sync_active_change(&mut self, was_active: bool) {
        let is_active = self.current_tab_is_active();
        if was_active != is_active
            && let Some(current_tab) = self.current_tab.as_mut()
        {
            if !is_active {
                current_tab.clear_active_focus();
            }
            current_tab.seq = current_tab.seq.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use agg::AgentState;
    use agg::Cmd;
    use agg::TabIndicator;
    use assert2::assert;
    use pretty_assertions::assert_eq;
    use ytil_agents::agent::Agent;
    use ytil_agents::agent::AgentEventKind;
    use ytil_agents::agent::AgentEventPayload;

    use super::*;
    use crate::wasm::state::current_tab::FocusedPaneLabel;
    use crate::wasm::state::current_tab::PaneFocus;
    use crate::wasm::state::nudge::Nudge;
    use crate::wasm::state::test_support::*;

    #[test]
    fn test_agent_busy_clears_nudge_dedupe() {
        let mut state = State {
            current_tab: Some(CurrentTab::new(10)),
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };
        assert!(let Some(current_tab) = state.current_tab.as_mut());
        current_tab.cwd = Some(PathBuf::from("/Users/me/project"));
        current_tab.pane_ids.insert(42);
        current_tab.pane_state_by_pane.insert(
            42,
            pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
        );

        let idle_events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Idle,
        });
        let _ = state.apply_all(&idle_events);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(Nudge::new(current_tab, &state.all_tabs, &state.home_dir, 42).is_some());
        state.mark_nudged(42);

        let busy_events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Busy,
        });
        let _ = state.apply_all(&busy_events);
        assert!(state.nudged_pane_ids.is_empty());

        let idle_events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Idle,
        });
        let _ = state.apply_all(&idle_events);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(Nudge::new(current_tab, &state.all_tabs, &state.home_dir, 42).is_some());
    }

    #[test]
    fn test_focus_changed_acknowledges_unseen_attention() {
        let mut state = State {
            known_active_tab_id: Some(10),
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        assert!(let Some(current_tab) = state.current_tab.as_mut());
        current_tab.pane_ids.extend([42, 43]);
        current_tab.focused_pane = Some(FocusedPane {
            id: 43,
            label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
        });
        current_tab.active_focus_pane_id = Some(43);
        current_tab.pane_state_by_pane.insert(
            42,
            pane_state(Agent::Codex, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
        );

        let events = vec![StateEvent::FocusChanged {
            new_pane: Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
            }),
            acknowledge_existing_attention: true,
        }];
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_focus_changed_to_seen_attention_with_running_peer_transitions_red_to_green() {
        let mut state = State {
            known_active_tab_id: Some(10),
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        assert!(let Some(current_tab) = state.current_tab.as_mut());
        current_tab.pane_ids.extend([42, 43]);
        current_tab.focused_pane = Some(FocusedPane {
            id: 43,
            label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
        });
        current_tab.active_focus_pane_id = Some(43);
        current_tab.pane_state_by_pane.insert(
            42,
            pane_state(Agent::Codex, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
        );
        current_tab.pane_state_by_pane.insert(
            43,
            pane_state(Agent::Claude, AgentPanePhase::Running, PaneFocus::Focused, 2),
        );

        let events = vec![StateEvent::FocusChanged {
            new_pane: Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
            }),
            acknowledge_existing_attention: true,
        }];
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Green);
        assert_eq!(current_tab.display_cmd(), Cmd::agent(Agent::Claude, AgentState::Busy));
    }

    #[test]
    fn test_running_resets_seen_attention_to_green() {
        let mut state = State {
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        assert!(let Some(current_tab) = state.current_tab.as_mut());
        current_tab.pane_ids.insert(42);
        current_tab.pane_state_by_pane.insert(
            42,
            pane_state(Agent::Codex, AgentPanePhase::AttentionSeen, PaneFocus::Unfocused, 1),
        );

        let events = vec![StateEvent::AgentBusy {
            pane_id: 42,
            agent: Agent::Codex,
        }];
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Green);
        assert_eq!(current_tab.display_cmd(), Cmd::agent(Agent::Codex, AgentState::Busy));
    }

    #[test]
    fn test_agent_lost_removes_unseen_attention() {
        let mut state = State {
            current_tab: Some(CurrentTab {
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = vec![StateEvent::AgentLost { pane_id: 42 }];
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::None);
        assert_eq!(current_tab.display_cmd(), Cmd::None);
    }
}
