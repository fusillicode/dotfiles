use crate::plugin::tbar::Event;
use crate::plugin::tbar::TbarState;

pub fn derive(state: &TbarState, pane_id: u32) -> Vec<Event> {
    let Some(current_tab) = state.current_tab.as_ref() else {
        return vec![];
    };
    if !current_tab.pane_state_by_pane.contains_key(&pane_id) {
        return vec![];
    }
    vec![Event::AgentLost { pane_id }]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ytil_agents::agent::Agent;

    use crate::plugin::tbar::Event;
    use crate::plugin::tbar::TbarState;
    use crate::plugin::tbar::current_tab::AgentPanePhase;
    use crate::plugin::tbar::current_tab::CurrentTab;
    use crate::plugin::tbar::current_tab::PaneFocus;
    use crate::plugin::tbar::events_from::pane_close::*;
    use crate::plugin::tbar::test_support::*;

    #[test]
    fn test_pane_close_removes_tracked_agent_immediately() {
        let state = TbarState {
            current_tab: Some(CurrentTab {
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Cursor, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        pretty_assertions::assert_eq!(derive(&state, 42), vec![Event::AgentLost { pane_id: 42 }]);
    }
}
