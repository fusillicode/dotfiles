use zellij_tile::prelude::TabInfo;

use crate::plugin::events::StateEvent;
use crate::plugin::state::State;
use crate::plugin::state::current_tab::FocusedPane;

pub fn derive(state: &State, new_tabs: &mut [TabInfo], landing_focus: Option<FocusedPane>) -> Vec<StateEvent> {
    new_tabs.sort_by_key(|tab| tab.position);

    let prev_tabs = &state.all_tabs;
    let mut events = vec![StateEvent::AllTabsReplaced {
        new_tabs: new_tabs.to_vec(),
    }];

    if crate::plugin::state::tabs::topology_changed(prev_tabs, new_tabs) {
        events.push(StateEvent::TopologyChanged);
    }

    let was_active = crate::plugin::state::queries::current_tab_is_active_in(prev_tabs, state.current_tab_id());
    let is_active = crate::plugin::state::queries::current_tab_is_active_in(new_tabs, state.current_tab_id());
    if !was_active && is_active {
        crate::plugin::state::events_from::push_became_active(&mut events, landing_focus);
    }

    let has_remap = crate::plugin::state::tabs::detect_remapped_tab_id(state.current_tab.as_ref(), prev_tabs, new_tabs)
        .is_some_and(|new_tab_id| {
            events.push(StateEvent::TabRemapped { new_tab_id });
            true
        });

    if state.current_tab.is_some()
        && (!state.sync_requested || crate::plugin::state::tabs::topology_changed(prev_tabs, new_tabs) || has_remap)
    {
        events.push(StateEvent::SyncRequested);
    }

    events
}
