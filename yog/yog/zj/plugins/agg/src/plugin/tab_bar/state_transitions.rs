use std::collections::HashSet;

use zellij_tile::prelude::TabInfo;

use crate::plugin::pane::FocusedPane;
use crate::plugin::tab_bar::Event;
use crate::plugin::tab_bar::StateSnapshotPayload;
use crate::plugin::tab_bar::TabBarState;
use crate::plugin::tab_bar::current_tab::AgentPanePhase;
use crate::plugin::tab_bar::current_tab::CurrentTab;

impl TabBarState {
    pub fn apply_all(&mut self, events: &[Event]) -> bool {
        for event in events {
            self.apply(event);
        }
        self.prune_nudges();
        self.sync_frame()
    }

    fn apply(&mut self, event: &Event) {
        match event {
            Event::TabCreated { tab_id } => {
                self.current_tab = Some(CurrentTab::new(*tab_id));
            }
            Event::TabRemapped { new_tab_id } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.tab_id = *new_tab_id;
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            Event::PanesChanged {
                observed_pane_ids,
                retained_pane_ids,
            } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.apply_panes_changed(observed_pane_ids, retained_pane_ids);
                }
            }
            Event::FocusChanged {
                new_pane,
                acknowledge_existing_attention,
            } => self.apply_focus_changed(new_pane.as_ref(), *acknowledge_existing_attention),
            Event::CwdChanged { pane_id, new_cwd } => {
                self.cwds_by_pane.insert(*pane_id, new_cwd.clone());
                if let Some(current_tab) = self.current_tab.as_mut() {
                    let is_display_pane = current_tab
                        .focused_pane
                        .as_ref()
                        .map(|focused_pane| focused_pane.id)
                        .or(current_tab.active_focus_pane_id)
                        == Some(*pane_id);
                    if is_display_pane && current_tab.cwd.as_ref() != Some(new_cwd) {
                        current_tab.cwd = Some(new_cwd.clone());
                        current_tab.seq = current_tab.seq.saturating_add(1);
                    }
                }
            }
            Event::AgentDetected { pane_id, agent } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.transition_phase(*pane_id, *agent, AgentPanePhase::AttentionSeen);
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            Event::AgentBusy { pane_id, agent } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.transition_phase(*pane_id, *agent, AgentPanePhase::Running);
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            Event::AgentIdle { pane_id, agent } => {
                let current_tab_is_active = self.current_tab_is_active();
                if let Some(current_tab) = self.current_tab.as_mut() {
                    let phase = crate::plugin::tab_bar::current_tab::idle_phase_for_pane(
                        current_tab,
                        current_tab_is_active,
                        *pane_id,
                    );
                    current_tab.transition_phase(*pane_id, *agent, phase);
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            Event::AgentLost { pane_id } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.apply_agent_lost(*pane_id);
                }
            }
            Event::GitStatChanged { new_stat } => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.git_stat = new_stat.clone();
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            Event::RemoteTabUpdated {
                source_plugin_id,
                snapshot,
                evict_ids,
            } => self.apply_remote_tab_updated(*source_plugin_id, snapshot, evict_ids),
            Event::ActiveTabChanged { active_tab_id } => self.apply_active_tab_changed(*active_tab_id),
            Event::AllTabsReplaced { new_tabs } => self.apply_all_tabs_replaced(new_tabs),
            Event::SyncRequested => {
                self.sync_requested = true;
            }
            Event::BecameActive => {
                if let Some(current_tab) = self.current_tab.as_mut() {
                    current_tab.pending_activation_focus_ack = true;
                    current_tab.seq = current_tab.seq.saturating_add(1);
                }
            }
            Event::TopologyChanged => {}
        }
    }

    fn apply_focus_changed(&mut self, new_pane: Option<&FocusedPane>, acknowledge_existing_attention: bool) {
        let is_active = self.current_tab_is_active();
        let Some(current_tab) = self.current_tab.as_mut() else {
            return;
        };
        current_tab.focused_pane = new_pane.cloned();
        if let Some(cwd) = new_pane
            .and_then(|focused_pane| self.cwds_by_pane.get(&focused_pane.id))
            .cloned()
        {
            current_tab.cwd = Some(cwd);
        }
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

    use crate::plugin::nudge::Nudge;
    use crate::plugin::pane::FocusedPaneLabel;
    use crate::plugin::tab_bar::current_tab::PaneFocus;
    use crate::plugin::tab_bar::state_transitions::*;
    use crate::plugin::tab_bar::test_support::*;

    #[test]
    fn test_agent_busy_clears_nudge_dedupe() {
        let mut state = TabBarState {
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

        let idle_events = crate::plugin::tab_bar::events_from::agent::derive(
            &state,
            &AgentEventPayload {
                pane_id: 42,
                agent: Agent::Codex,
                kind: AgentEventKind::Idle,
            },
        );
        let _ = state.apply_all(&idle_events);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(Nudge::new(current_tab, &state.all_tabs, &state.home_dir, 42).is_some());
        state.mark_nudged(42);

        let busy_events = crate::plugin::tab_bar::events_from::agent::derive(
            &state,
            &AgentEventPayload {
                pane_id: 42,
                agent: Agent::Codex,
                kind: AgentEventKind::Busy,
            },
        );
        let _ = state.apply_all(&busy_events);
        assert!(state.nudged_pane_ids.is_empty());

        let idle_events = crate::plugin::tab_bar::events_from::agent::derive(
            &state,
            &AgentEventPayload {
                pane_id: 42,
                agent: Agent::Codex,
                kind: AgentEventKind::Idle,
            },
        );
        let _ = state.apply_all(&idle_events);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(Nudge::new(current_tab, &state.all_tabs, &state.home_dir, 42).is_some());
    }

    #[test]
    fn test_focus_changed_uses_cached_cwd_for_display_pane() {
        let mut state = TabBarState {
            current_tab: Some(CurrentTab::new(10)),
            cwds_by_pane: HashMap::from([(42, PathBuf::from("/Users/me/project"))]),
            ..Default::default()
        };

        let _ = state.apply_all(&[Event::FocusChanged {
            new_pane: Some(FocusedPane { id: 42, label: None }),
            acknowledge_existing_attention: false,
        }]);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.cwd, Some(PathBuf::from("/Users/me/project")));
    }

    #[test]
    fn test_focus_changed_acknowledges_unseen_attention() {
        let mut state = TabBarState {
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

        let events = vec![Event::FocusChanged {
            new_pane: Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
            }),
            acknowledge_existing_attention: true,
        }];
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Seen);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_focus_changed_to_seen_attention_with_running_peer_transitions_unseen_to_busy() {
        let mut state = TabBarState {
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

        let events = vec![Event::FocusChanged {
            new_pane: Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
            }),
            acknowledge_existing_attention: true,
        }];
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Busy);
        assert_eq!(current_tab.display_cmd(), Cmd::agent(Agent::Claude, AgentState::Busy));
    }

    #[test]
    fn test_running_resets_seen_attention_to_busy() {
        let mut state = TabBarState {
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        assert!(let Some(current_tab) = state.current_tab.as_mut());
        current_tab.pane_ids.insert(42);
        current_tab.pane_state_by_pane.insert(
            42,
            pane_state(Agent::Codex, AgentPanePhase::AttentionSeen, PaneFocus::Unfocused, 1),
        );

        let events = vec![Event::AgentBusy {
            pane_id: 42,
            agent: Agent::Codex,
        }];
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Busy);
        assert_eq!(current_tab.display_cmd(), Cmd::agent(Agent::Codex, AgentState::Busy));
    }

    #[test]
    fn test_agent_lost_removes_unseen_attention() {
        let mut state = TabBarState {
            current_tab: Some(CurrentTab {
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = vec![Event::AgentLost { pane_id: 42 }];
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::NoAgent);
        assert_eq!(current_tab.display_cmd(), Cmd::None);
    }

    #[test]
    fn test_mat_requires_each_pane_focus_to_clear_unseen() {
        let mut state = TabBarState {
            known_active_tab_id: Some(10),
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                pane_state_by_pane: HashMap::from([
                    (
                        42,
                        pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                    ),
                    (
                        43,
                        pane_state(Agent::Cursor, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 2),
                    ),
                ]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events_a = vec![Event::FocusChanged {
            new_pane: Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
            acknowledge_existing_attention: true,
        }];
        let _ = state.apply_all(&events_a);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Unseen);

        let events_b = vec![Event::FocusChanged {
            new_pane: Some(FocusedPane {
                id: 43,
                label: Some(FocusedPaneLabel::TerminalCommand("cursor".to_string())),
            }),
            acknowledge_existing_attention: true,
        }];
        let _ = state.apply_all(&events_b);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Seen);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Cursor, AgentState::Acknowledged)
        );
    }
}
