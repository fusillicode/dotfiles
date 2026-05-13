use crate::plugin::ppick::state::PpickEvent;

pub fn derive(pane_id: u32) -> Vec<PpickEvent> {
    vec![PpickEvent::PaneRemoved { pane_id }]
}
