use std::path::PathBuf;

use crate::plugin::ppick::state::PpickEvent;

pub fn derive(pane_id: u32, cwd: PathBuf) -> Vec<PpickEvent> {
    vec![PpickEvent::CwdUpdated { pane_id, cwd }]
}
