use std::path::PathBuf;

use crate::plugin::picker::state::PickerEvent;
use crate::plugin::picker::state::PickerState;

pub fn derive(state: &PickerState, pane_id: u32, cwd: PathBuf) -> Vec<PickerEvent> {
    crate::plugin::picker::events_from::picker_event(state, PickerEvent::CwdUpdated { pane_id, cwd })
}
