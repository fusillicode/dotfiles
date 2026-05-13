use crate::plugin::ppick::state::PpickEvent;

pub fn derive(pane_id: u32, command: Vec<String>) -> Vec<PpickEvent> {
    vec![PpickEvent::CommandUpdated { pane_id, command }]
}
