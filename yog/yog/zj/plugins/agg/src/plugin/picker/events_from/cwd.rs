use std::path::PathBuf;

use crate::plugin::picker::state::PickerEvent;

pub fn derive(pane_id: u32, cwd: PathBuf) -> Vec<PickerEvent> {
    vec![PickerEvent::CwdUpdated { pane_id, cwd }]
}
