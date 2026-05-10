use crate::plugin::picker::state::PickerEvent;
use crate::plugin::picker::state::PickerState;

pub mod agent;
pub mod command;
pub mod cwd;
pub mod git_stat;
pub mod key;
pub mod pane_close;
pub mod pane_update;
pub mod sessions;
pub mod tab_update;

pub fn picker_event(state: &PickerState, event: PickerEvent) -> Vec<PickerEvent> {
    state.event_would_change(&event).then_some(event).into_iter().collect()
}
