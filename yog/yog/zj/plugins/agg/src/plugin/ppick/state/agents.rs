use ytil_agents::agent::AgentEventPayload;

use crate::plugin::ppick::state::PpickState;
use crate::plugin::tbar::PaneAgentSnapshot;
use crate::plugin::tbar::StateSnapshotPayload;

impl PpickState {
    pub fn apply_state_snapshot(&mut self, snapshot: &StateSnapshotPayload) -> bool {
        let mut changed = snapshot
            .focused_pane_id
            .is_some_and(|pane_id| self.set_initial_focus_pane(snapshot.tab_id, pane_id, snapshot.seq));
        changed |= self.update_agent_snapshots(&snapshot.pane_agents);
        changed
    }

    pub fn update_agent(&mut self, event: &AgentEventPayload) -> bool {
        let Some(entry) = self
            .pane_entries
            .iter_mut()
            .find(|entry| entry.pane_id == event.pane_id)
        else {
            return false;
        };
        let mut changed = entry.apply_agent_event(event);
        self.agent_snapshots_by_pane.remove(&event.pane_id);
        if changed {
            self.mark_filter_dirty();
            changed |= self.clamp_selection();
        }
        changed
    }

    pub fn update_agent_snapshots(&mut self, snapshots: &[PaneAgentSnapshot]) -> bool {
        let mut changed = false;
        for snapshot in snapshots {
            if self.agent_snapshots_by_pane.get(&snapshot.pane_id) != Some(snapshot) {
                self.agent_snapshots_by_pane.insert(snapshot.pane_id, *snapshot);
            }
            if let Some(entry) = self
                .pane_entries
                .iter_mut()
                .find(|entry| entry.pane_id == snapshot.pane_id)
            {
                changed |= entry.apply_agent_snapshot(*snapshot);
            }
        }
        if changed {
            self.mark_filter_dirty();
            changed |= self.clamp_selection();
        }
        changed
    }
}
