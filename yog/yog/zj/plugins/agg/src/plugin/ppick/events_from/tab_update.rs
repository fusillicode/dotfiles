use zellij_tile::prelude::TabInfo;

use crate::plugin::ppick::state::PpickEvent;

pub fn derive(tabs: Vec<TabInfo>) -> Vec<PpickEvent> {
    vec![PpickEvent::TabsUpdated { tabs }]
}
