use crate::plugin::picker::state::PickerEvent;

pub fn derive(pane_id: u32) -> Vec<PickerEvent> {
    vec![PickerEvent::PaneRemoved { pane_id }]
}
