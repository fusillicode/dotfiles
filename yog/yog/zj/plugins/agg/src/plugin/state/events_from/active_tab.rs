use crate::plugin::events::StateEvent;
use crate::plugin::state::State;
use crate::plugin::state::current_tab::AgentPanePhase;
use crate::plugin::state::current_tab::CurrentTab;
use crate::plugin::state::current_tab::FocusedPane;

pub fn derive(state: &State, active_tab_id: usize, landing_focus: Option<FocusedPane>) -> Vec<StateEvent> {
    if state.known_active_tab_id == Some(active_tab_id) {
        if let Some(event) = same_tab_activation_focus_event(state.current_tab.as_ref(), active_tab_id, landing_focus) {
            return vec![event];
        }
        return vec![];
    }

    let mut events = vec![StateEvent::ActiveTabChanged { active_tab_id }];
    let was_active = state.current_tab_is_active();
    let is_active = state.current_tab_id() == Some(active_tab_id);
    if !was_active && is_active {
        crate::plugin::state::events_from::push_became_active(&mut events, landing_focus);
    }

    events
}

fn same_tab_activation_focus_event(
    current_tab: Option<&CurrentTab>,
    active_tab_id: usize,
    landing_focus: Option<FocusedPane>,
) -> Option<StateEvent> {
    let current_tab = current_tab?;
    if current_tab.tab_id != active_tab_id {
        return None;
    }

    let landing_focus = landing_focus?;
    should_reconcile_same_tab_activation_focus(current_tab, &landing_focus).then_some(StateEvent::FocusChanged {
        new_pane: Some(landing_focus),
        acknowledge_existing_attention: true,
    })
}

fn should_reconcile_same_tab_activation_focus(current_tab: &CurrentTab, landing_focus: &FocusedPane) -> bool {
    let landing_focus_id = landing_focus.id;
    current_tab.pending_activation_focus_ack
        || current_tab.active_focus_pane_id != Some(landing_focus_id)
        || current_tab.focused_pane.as_ref().map(|pane| pane.id) != Some(landing_focus_id)
        || current_tab
            .pane_state_by_pane
            .get(&landing_focus_id)
            .is_some_and(|pane_state| pane_state.phase == AgentPanePhase::AttentionUnseen)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use agg::AgentState;
    use agg::Cmd;
    use agg::TabIndicator;
    use assert2::assert;
    use pretty_assertions::assert_eq;
    use ytil_agents::agent::Agent;
    use zellij_tile::prelude::TabInfo;

    use super::*;
    use crate::plugin::events::StateEvent;
    use crate::plugin::state::State;
    use crate::plugin::state::current_tab::AgentPanePhase;
    use crate::plugin::state::current_tab::CurrentTab;
    use crate::plugin::state::current_tab::FocusedPane;
    use crate::plugin::state::current_tab::FocusedPaneLabel;
    use crate::plugin::state::current_tab::PaneFocus;
    use crate::plugin::state::test_support::*;

    #[test]
    fn test_active_tab_change_restores_focus_and_acknowledges_landing_unseen_attention() {
        let mut state = State {
            known_active_tab_id: Some(20),
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = derive(
            &state,
            10,
            Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
        );
        assert_eq!(
            events,
            vec![
                StateEvent::ActiveTabChanged { active_tab_id: 10 },
                StateEvent::BecameActive,
                StateEvent::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                    }),
                    acknowledge_existing_attention: true,
                },
            ]
        );

        let _ = state.apply_all(&events);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(current_tab.active_focus_pane_id, Some(42));
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&42)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionSeen)
        );
    }

    #[test]
    fn test_active_tab_change_keeps_red_when_landing_on_other_pane() {
        let mut state = State {
            known_active_tab_id: Some(20),
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
                }),
                pane_state_by_pane: HashMap::from([
                    (
                        42,
                        pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                    ),
                    (
                        43,
                        pane_state(Agent::Codex, AgentPanePhase::AttentionSeen, PaneFocus::Unfocused, 2),
                    ),
                ]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = derive(
            &state,
            10,
            Some(FocusedPane {
                id: 43,
                label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
            }),
        );
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);
        assert_eq!(current_tab.active_focus_pane_id, Some(43));
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&42)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionUnseen)
        );
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&43)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionSeen)
        );
        assert_eq!(
            current_tab.current_row_display(true),
            (Cmd::agent(Agent::Claude, AgentState::NeedsAttention), TabIndicator::Red,)
        );
    }

    #[test]
    fn test_active_tab_change_without_host_focus_acknowledges_matching_pane_on_first_pane_update() {
        let mut state = State {
            plugin_id: 7,
            known_active_tab_id: Some(20),
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let activation_events = derive(&state, 10, None);
        assert_eq!(
            activation_events,
            vec![
                StateEvent::ActiveTabChanged { active_tab_id: 10 },
                StateEvent::BecameActive,
            ]
        );
        let _ = state.apply_all(&activation_events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(current_tab.pending_activation_focus_ack);
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);

        let pane_update_events = apply_pane_update(
            &mut state,
            &manifest(vec![(
                0,
                vec![
                    plugin_pane(7),
                    terminal_pane_with_command(42, true, "claude"),
                    terminal_pane_with_command(43, false, "/bin/zsh"),
                ],
            )]),
        );
        assert_eq!(
            pane_update_events,
            vec![
                StateEvent::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                    }),
                    acknowledge_existing_attention: true,
                },
                StateEvent::SyncRequested,
            ]
        );

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(!current_tab.pending_activation_focus_ack);
        assert_eq!(current_tab.active_focus_pane_id, Some(42));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&42)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionSeen)
        );
    }

    #[test]
    fn test_active_tab_pipe_acknowledges_landing_focus_after_tab_update_pending_ack() {
        let mut state = State {
            known_active_tab_id: Some(20),
            all_tabs: vec![TabInfo {
                tab_id: 20,
                position: 1,
                active: true,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
                }),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let mut tabs = vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..tab_with_name(10, 0, "a")
        }];
        let tab_update_events = crate::plugin::state::events_from::tab_update::derive(&state, &mut tabs, None);
        assert_eq!(
            tab_update_events,
            vec![
                StateEvent::AllTabsReplaced { new_tabs: tabs.clone() },
                StateEvent::TopologyChanged,
                StateEvent::BecameActive,
                StateEvent::SyncRequested,
            ]
        );
        let _ = state.apply_all(&tab_update_events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(current_tab.pending_activation_focus_ack);
        assert_eq!(state.known_active_tab_id, Some(10));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);

        let pipe_events = derive(
            &state,
            10,
            Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
        );
        assert_eq!(
            pipe_events,
            vec![StateEvent::FocusChanged {
                new_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                acknowledge_existing_attention: true,
            }]
        );
        let _ = state.apply_all(&pipe_events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(!current_tab.pending_activation_focus_ack);
        assert_eq!(current_tab.active_focus_pane_id, Some(42));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&42)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionSeen)
        );
    }

    #[test]
    fn test_active_tab_pipe_reconciles_real_landing_focus_after_stale_tab_update_focus() {
        let mut state = State {
            known_active_tab_id: Some(20),
            all_tabs: vec![TabInfo {
                tab_id: 20,
                position: 1,
                active: true,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
                }),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let mut tabs = vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..tab_with_name(10, 0, "a")
        }];
        let tab_update_events = crate::plugin::state::events_from::tab_update::derive(
            &state,
            &mut tabs,
            Some(FocusedPane {
                id: 43,
                label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
            }),
        );
        let _ = state.apply_all(&tab_update_events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(!current_tab.pending_activation_focus_ack);
        assert_eq!(current_tab.active_focus_pane_id, Some(43));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);

        let pipe_events = derive(
            &state,
            10,
            Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
        );
        assert_eq!(
            pipe_events,
            vec![StateEvent::FocusChanged {
                new_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                acknowledge_existing_attention: true,
            }]
        );
        let _ = state.apply_all(&pipe_events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.active_focus_pane_id, Some(42));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&42)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionSeen)
        );
    }

    #[test]
    fn test_active_tab_pipe_matching_landing_focus_with_no_unseen_attention_is_noop() {
        let state = State {
            known_active_tab_id: Some(10),
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                active_focus_pane_id: Some(42),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionSeen, PaneFocus::Focused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = derive(
            &state,
            10,
            Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
        );

        assert_eq!(events, vec![]);
    }
}
