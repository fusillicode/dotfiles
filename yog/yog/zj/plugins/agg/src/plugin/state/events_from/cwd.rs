use std::path::PathBuf;

use crate::plugin::events::StateEvent;
use crate::plugin::state::State;

pub fn derive(state: &State, pane_id: u32, cwd: PathBuf) -> Vec<StateEvent> {
    let Some(current_tab) = state.current_tab.as_ref() else {
        return vec![];
    };
    if current_tab.focused_pane.as_ref().map(|focused_pane| focused_pane.id) != Some(pane_id) {
        return vec![];
    }
    if current_tab.cwd.as_ref() == Some(&cwd) {
        return vec![];
    }
    vec![StateEvent::CwdChanged { new_cwd: cwd }]
}
