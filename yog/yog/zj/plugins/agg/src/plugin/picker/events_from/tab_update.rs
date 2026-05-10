use zellij_tile::prelude::TabInfo;

use crate::plugin::picker::state::PickerEvent;
use crate::plugin::picker::state::PickerState;

pub fn derive(state: &PickerState, tabs: Vec<TabInfo>) -> Vec<PickerEvent> {
    crate::plugin::picker::events_from::picker_event(state, PickerEvent::TabsUpdated { tabs })
}
