use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::TabSnapshot;
use muxr_core::TerminalSize;
use rootcause::report;

use crate::layout::Pane;
use crate::layout::PaneFocusDirection;
use crate::layout::PaneResizeDirection;
use crate::layout::PaneSplitAxis;
use crate::layout::Tab;
use crate::layout::region::PaneLayout;
use crate::pty::PtyExitStatus;

impl Tab {
    pub fn snapshot(&self) -> rootcause::Result<TabSnapshot> {
        let panes = self.panes().into_iter().map(Pane::snapshot).collect();
        TabSnapshot::new(self.id.clone(), self.title.clone(), self.active_pane.clone(), panes)
    }

    pub fn split_active_pane(&mut self, new_pane: &Pane, split_axis: PaneSplitAxis) -> rootcause::Result<()> {
        if !self.pane_tree.split_pane(&self.active_pane, new_pane, split_axis)? {
            return Err(report!("muxr active pane is missing from server layout")
                .attach(format!("active_pane={}", self.active_pane)));
        }
        Ok(())
    }

    pub fn resize_active_pane(&mut self, direction: PaneResizeDirection) -> rootcause::Result<bool> {
        self.pane_tree.resize_pane(&self.active_pane, direction)
    }

    pub fn focus_pane_at(&mut self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<bool> {
        let Some(pane_id) = self.pane_at(size, position)? else {
            return Ok(false);
        };

        self.focus_pane(pane_id)
    }

    pub fn focus_pane_direction(
        &mut self,
        size: &TerminalSize,
        direction: PaneFocusDirection,
    ) -> rootcause::Result<bool> {
        let layout = self.pane_layout(size)?;
        let active_region = layout
            .regions()
            .iter()
            .find(|region| region.id() == &self.active_pane)
            .ok_or_else(|| {
                report!("muxr active pane is missing from active tab layout")
                    .attach(format!("active_pane={}", self.active_pane))
            })?;
        let Some(next_pane_id) = layout
            .regions()
            .iter()
            .filter(|region| region.id() != active_region.id())
            .filter(|region| region.is_adjacent_to(active_region, direction))
            .max_by_key(|region| region.focus_seq())
            .map(|region| region.id().clone())
        else {
            return Ok(false);
        };

        self.focus_pane(next_pane_id)
    }

    pub fn focus_pane(&mut self, pane_id: PaneId) -> rootcause::Result<bool> {
        if self.active_pane == pane_id {
            return Ok(false);
        }

        let focus_seq = self.next_focus_seq()?;
        let Some(pane) = self.pane_tree.pane_mut(&pane_id) else {
            return Err(report!("muxr pane is missing from active tab").attach(format!("pane_id={pane_id}")));
        };
        pane.focus_seq = focus_seq;
        self.active_pane = pane_id;
        Ok(true)
    }

    pub fn pane_at(&self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<Option<PaneId>> {
        Ok(self
            .pane_layout(size)?
            .regions()
            .iter()
            .find(|region| region.contains(position.row, position.col))
            .map(|region| region.id().clone()))
    }

    pub fn remove_pane(&mut self, pane_id: &PaneId) -> rootcause::Result<PaneId> {
        self.pane_tree.remove_pane(pane_id)
    }

    pub fn mark_pane_exited(
        &mut self,
        pane_id: &PaneId,
        exited_at: u64,
        exit_status: Option<PtyExitStatus>,
    ) -> rootcause::Result<()> {
        let Some(pane) = self.pane_tree.pane_mut(pane_id) else {
            return Err(report!("muxr pane is missing from server layout").attach(format!("pane_id={pane_id}")));
        };

        pane.exited_at = Some(exited_at);
        pane.exit_status = exit_status;
        Ok(())
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
            .map(|pane| pane.focus_seq)
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
