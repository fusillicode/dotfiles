use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::TabId;
use muxr_core::TabSnapshot;
use muxr_core::TerminalSize;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

use crate::geometry::PaneLayout;
use crate::state::Pane;
use crate::state::PaneNode;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Tab {
    pub active_pane: PaneId,
    pub id: TabId,
    pub pane_tree: PaneNode,
    pub title: String,
}

impl Tab {
    pub fn snapshot(&self) -> rootcause::Result<TabSnapshot> {
        let panes = self.panes().into_iter().map(Pane::snapshot).collect();
        TabSnapshot::new(self.id.clone(), self.title.clone(), self.active_pane.clone(), panes)
    }

    pub fn pane_at(&self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<Option<PaneId>> {
        Ok(self
            .pane_layout(size)?
            .regions()
            .iter()
            .find(|region| region.contains(position.row, position.col))
            .map(|region| region.id().clone()))
    }

    pub fn pane_count(&self) -> usize {
        self.pane_tree.pane_count()
    }

    pub fn contains_pane(&self, pane_id: &PaneId) -> bool {
        self.pane_tree.contains_pane(pane_id)
    }

    pub fn pane_ids(&self) -> Vec<&str> {
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
            .map(|pane| pane.focus_seq())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| report!("muxr pane focus sequence overflowed"))
    }

    pub fn pane_layout(&self, size: &TerminalSize) -> rootcause::Result<PaneLayout> {
        let mut layout = PaneLayout::default();
        self.pane_tree
            .append_layout(0, 0, size.rows(), size.cols(), &mut layout)?;
        Ok(layout)
    }
}
