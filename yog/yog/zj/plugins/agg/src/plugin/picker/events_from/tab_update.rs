use zellij_tile::prelude::TabInfo;

use crate::plugin::picker::state::PickerEvent;

pub fn derive(tabs: Vec<TabInfo>) -> Vec<PickerEvent> {
    vec![PickerEvent::TabsUpdated { tabs }]
}
