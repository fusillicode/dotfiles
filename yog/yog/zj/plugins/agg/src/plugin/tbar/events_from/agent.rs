use ytil_agents::agent::AgentEventKind;
use ytil_agents::agent::AgentEventPayload;

use crate::plugin::tbar::Event;
use crate::plugin::tbar::TbarState;
use crate::plugin::tbar::current_tab::AgentPanePhase;

pub fn derive(state: &TbarState, event: &AgentEventPayload) -> Vec<Event> {
    let Some(current_tab) = state.current_tab.as_ref() else {
        return vec![];
    };
    if !current_tab.pane_ids.contains(&event.pane_id) {
        return vec![];
    }

    let current_pane_state = current_tab.pane_state_by_pane.get(&event.pane_id);
    if current_pane_state.is_some_and(|pane_state| event.agent.priority() < pane_state.agent.priority()) {
        return vec![];
    }

    let pane_id = event.pane_id;
    let agent = event.agent;
    let current_tab_is_active = state.current_tab_is_active();
    match event.kind {
        AgentEventKind::Start => {
            if current_pane_state.is_some_and(|pane_state| pane_state.agent == agent) {
                return vec![];
            }
            vec![Event::AgentDetected { pane_id, agent }]
        }
        AgentEventKind::Busy => {
            if current_pane_state
                .is_some_and(|pane_state| pane_state.agent == agent && pane_state.phase == AgentPanePhase::Running)
            {
                return vec![];
            }
            vec![Event::AgentBusy { pane_id, agent }]
        }
        AgentEventKind::Idle => {
            let desired_phase =
                crate::plugin::tbar::current_tab::idle_phase_for_pane(current_tab, current_tab_is_active, pane_id);
            if current_pane_state
                .is_some_and(|pane_state| pane_state.agent == agent && pane_state.phase == desired_phase)
            {
                return vec![];
            }
            vec![Event::AgentIdle { pane_id, agent }]
        }
        AgentEventKind::Exit => {
            if current_pane_state.is_none() {
                return vec![];
            }
            vec![Event::AgentLost { pane_id }]
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
    use ytil_agents::agent::Agent;
    use ytil_agents::agent::AgentEventKind;
    use ytil_agents::agent::AgentEventPayload;

    use crate::plugin::nudge::Nudge;
    use crate::plugin::pane::FocusedPane;
    use crate::plugin::pane::FocusedPaneLabel;
    use crate::plugin::tbar::Event;
    use crate::plugin::tbar::TbarState;
    use crate::plugin::tbar::current_tab::AgentPanePhase;
    use crate::plugin::tbar::current_tab::CurrentTab;
    use crate::plugin::tbar::current_tab::PaneFocus;
    use crate::plugin::tbar::events_from::agent::*;
    use crate::plugin::tbar::test_support::*;

    #[test]
    fn test_agent_start_sets_seen_indicator() {
        let mut state = TbarState {
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        assert2::assert!(let Some(current_tab) = state.current_tab.as_mut());
        current_tab.pane_ids.insert(42);

        let events = derive(
            &state,
            &AgentEventPayload {
                pane_id: 42,
                agent: Agent::Codex,
                kind: AgentEventKind::Start,
            },
        );
        pretty_assertions::assert_eq!(
            events,
            vec![Event::AgentDetected {
                pane_id: 42,
                agent: Agent::Codex,
            }]
        );

        let _ = state.apply_all(&events);
        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Seen);
        pretty_assertions::assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_agent_idle_in_unfocused_pane_transitions_busy_to_unseen() {
        let mut state = TbarState {
            all_tabs: vec![tab_with_name(10, 0, "fallback-tab")],
            current_tab: Some(CurrentTab::new(10)),
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };
        assert2::assert!(let Some(current_tab) = state.current_tab.as_mut());
        current_tab.cwd = Some(PathBuf::from("/Users/me/project"));
        current_tab.pane_ids.extend([42, 43]);
        current_tab.focused_pane = Some(FocusedPane {
            id: 43,
            label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
        });
        current_tab.active_focus_pane_id = Some(43);
        current_tab.pane_state_by_pane.insert(
            42,
            pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
        );

        let events = derive(
            &state,
            &AgentEventPayload {
                pane_id: 42,
                agent: Agent::Codex,
                kind: AgentEventKind::Idle,
            },
        );
        pretty_assertions::assert_eq!(
            events,
            vec![Event::AgentIdle {
                pane_id: 42,
                agent: Agent::Codex,
            }]
        );

        let _ = state.apply_all(&events);
        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Unseen);
        pretty_assertions::assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::NeedsAttention)
        );

        let nudge = Nudge::new(current_tab, &state.all_tabs, &state.home_dir, 42);
        pretty_assertions::assert_eq!(
            nudge,
            Some(Nudge {
                agent: Agent::Codex,
                tab_id: 10,
                pane_id: 42,
                path: ytil_tui::short_path(&PathBuf::from("/Users/me/project"), &PathBuf::from("/Users/me")),
            })
        );
        let nudge = nudge.expect("nudge");
        pretty_assertions::assert_eq!(nudge.path, "~/project");
        let nudges = state.nudges();
        assert2::assert!(let [(42, nudge)] = nudges.as_slice());
        pretty_assertions::assert_eq!(nudge.agent, Agent::Codex);
        pretty_assertions::assert_eq!(nudge.tab_id, 10);
        pretty_assertions::assert_eq!(nudge.pane_id, 42);
        pretty_assertions::assert_eq!(
            nudge.path,
            ytil_tui::short_path(&PathBuf::from("/Users/me/project"), &PathBuf::from("/Users/me"))
        );
        assert2::assert!(!state.nudged_pane_ids.contains(&42));
        state.mark_nudged(42);
        assert2::assert!(state.nudged_pane_ids.contains(&42));
        assert2::assert!(state.nudges().is_empty());
    }

    #[test]
    fn test_agent_idle_in_focused_pane_transitions_busy_to_seen() {
        let mut state = TbarState {
            known_active_tab_id: Some(10),
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        assert2::assert!(let Some(current_tab) = state.current_tab.as_mut());
        current_tab.pane_ids.insert(42);
        current_tab.focused_pane = Some(FocusedPane {
            id: 42,
            label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
        });
        current_tab.active_focus_pane_id = Some(42);
        current_tab.pane_state_by_pane.insert(
            42,
            pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Focused, 1),
        );

        let events = derive(
            &state,
            &AgentEventPayload {
                pane_id: 42,
                agent: Agent::Codex,
                kind: AgentEventKind::Idle,
            },
        );
        let _ = state.apply_all(&events);

        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Seen);
        pretty_assertions::assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_agent_idle_in_inactive_tab_with_stale_focus_transitions_to_unseen() {
        let mut state = TbarState {
            known_active_tab_id: Some(20),
            all_tabs: vec![tab_with_name(10, 0, "a"), tab_with_name(20, 1, "b")],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
                }),
                active_focus_pane_id: Some(42),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Focused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = derive(
            &state,
            &AgentEventPayload {
                pane_id: 42,
                agent: Agent::Codex,
                kind: AgentEventKind::Idle,
            },
        );
        pretty_assertions::assert_eq!(
            events,
            vec![Event::AgentIdle {
                pane_id: 42,
                agent: Agent::Codex,
            }]
        );

        let _ = state.apply_all(&events);

        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Unseen);
        pretty_assertions::assert_eq!(
            current_tab.current_row_display(false),
            (
                Cmd::agent(Agent::Codex, AgentState::NeedsAttention),
                TabIndicator::Unseen,
            )
        );
    }

    #[test]
    fn test_attention_after_focus_restore_is_seen_immediately() {
        let mut state = TbarState {
            known_active_tab_id: Some(10),
            current_tab: Some(CurrentTab {
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                active_focus_pane_id: Some(42),
                pane_ids: std::iter::once(42).collect(),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = derive(
            &state,
            &AgentEventPayload {
                pane_id: 42,
                agent: Agent::Claude,
                kind: AgentEventKind::Idle,
            },
        );
        let _ = state.apply_all(&events);

        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Seen);
        pretty_assertions::assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Claude, AgentState::Acknowledged)
        );
    }
}
