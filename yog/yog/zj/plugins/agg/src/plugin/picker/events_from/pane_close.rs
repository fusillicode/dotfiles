use crate::plugin::picker::state::PickerEvent;
use crate::plugin::picker::state::PickerState;

pub fn derive(state: &PickerState, pane_id: u32) -> Vec<PickerEvent> {
    crate::plugin::picker::events_from::picker_event(state, PickerEvent::PaneRemoved { pane_id })
}
