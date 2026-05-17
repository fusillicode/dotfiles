use std::collections::HashMap;

use crate::plugin::ppick::entry::PaneEntry;
use crate::plugin::ppick::state::PpickState;
use crate::plugin::ppick::state::SessionEntry;

impl PpickState {
    pub fn update_sessions(&mut self, session_entries: Vec<SessionEntry>) -> bool {
        let next_sessions_by_key = index_session_entries(session_entries);
        let sessions_changed = self.sessions_by_key != next_sessions_by_key;
        self.sessions_by_key = next_sessions_by_key;
        let entries_changed = attach_sessions_to_entries(&mut self.pane_entries, &self.sessions_by_key);
        if entries_changed {
            self.mark_filter_dirty();
        }
        let selection_changed = self.clamp_selection();
        sessions_changed || entries_changed || selection_changed
    }
}

pub(super) fn attach_sessions_to_entries(
    pane_entries: &mut [PaneEntry],
    sessions_by_key: &HashMap<(String, String), SessionEntry>,
) -> bool {
    let mut changed = false;
    for entry in pane_entries {
        changed |= entry.attach_session(sessions_by_key);
    }
    changed
}

fn index_session_entries(session_entries: Vec<SessionEntry>) -> HashMap<(String, String), SessionEntry> {
    let mut sessions_by_key = HashMap::new();
    for session in session_entries {
        sessions_by_key.insert((session.agent.clone(), session.session_id.clone()), session);
    }
    sessions_by_key
}
