use zellij_tile::prelude::TabInfo;

use crate::plugin::tbar::TbarState;

impl TbarState {
    pub fn current_tab_is_active(&self) -> bool {
        let current_tab_id = self.current_tab_id();
        self.known_active_tab_id.map_or_else(
            || current_tab_is_active_in(&self.all_tabs, current_tab_id),
            |active_tab_id| current_tab_id == Some(active_tab_id),
        )
    }

    pub fn current_tab_id(&self) -> Option<usize> {
        self.current_tab.as_ref().map(|current_tab| current_tab.tab_id)
    }
}

pub fn current_tab_is_active_in(tabs: &[TabInfo], current_tab_id: Option<usize>) -> bool {
    current_tab_id.is_some_and(|id| tabs.iter().any(|tab| tab.active && tab.tab_id == id))
}
