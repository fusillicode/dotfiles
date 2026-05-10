use crate::plugin::picker::state::PickerEvent;
use crate::plugin::picker::state::PickerState;

pub fn derive(state: &PickerState, pane_id: u32, command: Vec<String>) -> Vec<PickerEvent> {
    crate::plugin::picker::events_from::picker_event(state, PickerEvent::CommandUpdated { pane_id, command })
}
