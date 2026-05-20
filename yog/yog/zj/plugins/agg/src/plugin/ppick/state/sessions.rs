use std::collections::BTreeSet;
use std::collections::HashMap;

use ytil_agents::agent::Agent;
use ytil_agents::agent::session::SessionKey;

use crate::plugin::ppick::entry::PaneEntry;
use crate::plugin::ppick::state::PpickState;
use crate::plugin::ppick::state::SessionEntry;

impl PpickState {
    pub fn is_current_session_request(&self, session_keys: &[SessionKey]) -> bool {
        self.requested_session_keys.as_slice() == session_keys
    }

    pub fn take_session_keys_to_request(&mut self) -> Vec<SessionKey> {
        let next_keys = self
            .pane_entries
            .iter()
            .filter_map(PaneEntry::session_key)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if next_keys.is_empty() {
            self.requested_session_keys.clear();
            return Vec::new();
        }
        if self.requested_session_keys == next_keys {
            return Vec::new();
        }
        self.requested_session_keys.clone_from(&next_keys);
        next_keys
    }

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
    sessions_by_key: &HashMap<SessionKey, SessionEntry>,
) -> bool {
    let mut changed = false;
    for entry in pane_entries {
        changed |= entry.attach_session(sessions_by_key);
    }
    changed
}

fn index_session_entries(session_entries: Vec<SessionEntry>) -> HashMap<SessionKey, SessionEntry> {
    let mut sessions_by_key = HashMap::new();
    for session in session_entries {
        let Ok(agent) = Agent::from_name(&session.agent) else {
            continue;
        };
        sessions_by_key.insert(SessionKey::new(agent, session.session_id.clone()), session);
    }
    sessions_by_key
}
