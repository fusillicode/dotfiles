use crate::plugin::picker::state::PickerEvent;

pub fn derive(pane_id: u32, command: Vec<String>) -> Vec<PickerEvent> {
    vec![PickerEvent::CommandUpdated { pane_id, command }]
}
