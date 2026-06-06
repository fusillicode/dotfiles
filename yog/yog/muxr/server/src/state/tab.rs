use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::TabId;
use muxr_core::TabSnapshot;
use muxr_core::TerminalSize;
use muxr_core::TrackedProcessState;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

use crate::pane_layout::PaneLayout;
use crate::state::Pane;
use crate::state::PaneTree;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Tab {
    pub active_pane: PaneId,
    pub id: TabId,
    pub pane_tree: PaneTree,
    pub title: String,
}

impl Tab {
    pub fn snapshot_with_runtime_metadata(
        &self,
        terminal_titles: &[(PaneId, Option<String>)],
        runtime_cmd_labels: &[(PaneId, Option<String>)],
        runtime_tracked_process_states: &[(PaneId, TrackedProcessState)],
    ) -> rootcause::Result<TabSnapshot> {
        let panes = self
            .panes()
            .into_iter()
            .map(|pane| {
                let terminal_title = terminal_titles
                    .iter()
                    .find(|(pane_id, _title)| pane_id == &pane.id)
                    .and_then(|(_pane_id, title)| title.as_deref());
                let runtime_cmd_label = runtime_cmd_labels
                    .iter()
                    .find(|(pane_id, _cmd_label)| pane_id == &pane.id)
                    .and_then(|(_pane_id, cmd_label)| cmd_label.as_deref());
                let runtime_tracked_process_state = runtime_tracked_process_states
                    .iter()
                    .find(|(pane_id, _tracked_process_state)| pane_id == &pane.id)
                    .map_or(TrackedProcessState::None, |(_pane_id, tracked_process_state)| {
                        *tracked_process_state
                    });
                pane.snapshot_with_runtime_metadata(terminal_title, runtime_cmd_label, runtime_tracked_process_state)
            })
            .collect();
        TabSnapshot::new(self.id, self.title.clone(), self.active_pane, panes)
    }

    pub fn pane_at(&self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<Option<PaneId>> {
        Ok(self
            .pane_layout(size)?
            .regions()
            .iter()
            .find(|region| region.contains(position.into()))
            .map(|region| region.id))
    }

    pub fn pane_count(&self) -> usize {
        self.pane_tree.pane_count()
    }

    pub fn contains_pane(&self, pane_id: PaneId) -> bool {
        self.pane_tree.contains_pane(pane_id)
    }

    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        self.pane_tree.append_pane_ids(&mut ids);
        ids
    }

    pub fn panes(&self) -> Vec<&Pane> {
        let mut panes = Vec::new();
        self.pane_tree.append_panes(&mut panes);
        panes
    }

    pub fn next_focus_seq(&self) -> rootcause::Result<u64> {
        self.panes()
            .iter()
            .map(|pane| pane.focus_seq)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| report!("muxr pane focus sequence overflowed"))
    }

    pub fn pane_layout(&self, size: &TerminalSize) -> rootcause::Result<PaneLayout> {
        PaneLayout::from_pane_tree(&self.pane_tree, size)
    }
}
