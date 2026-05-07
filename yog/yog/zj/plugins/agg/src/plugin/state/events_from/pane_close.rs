use crate::plugin::events::StateEvent;
use crate::plugin::state::State;

pub fn derive(state: &State, pane_id: u32) -> Vec<StateEvent> {
    let Some(current_tab) = state.current_tab.as_ref() else {
        return vec![];
    };
    if !current_tab.pane_state_by_pane.contains_key(&pane_id) {
        return vec![];
    }
    vec![StateEvent::AgentLost { pane_id }]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pretty_assertions::assert_eq;
    use ytil_agents::agent::Agent;

    use super::*;
    use crate::plugin::events::StateEvent;
    use crate::plugin::state::State;
    use crate::plugin::state::current_tab::AgentPanePhase;
    use crate::plugin::state::current_tab::CurrentTab;
    use crate::plugin::state::current_tab::PaneFocus;
    use crate::plugin::state::test_support::*;

    #[test]
    fn test_pane_close_removes_tracked_agent_immediately() {
        let state = State {
            current_tab: Some(CurrentTab {
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Cursor, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        assert_eq!(derive(&state, 42), vec![StateEvent::AgentLost { pane_id: 42 }]);
    }
}
