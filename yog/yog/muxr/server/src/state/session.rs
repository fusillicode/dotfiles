use std::collections::BTreeMap;

use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::SessionName;
use muxr_core::TabId;
use muxr_core::TerminalSize;
use muxr_core::TrackedProcessState;
use rootcause::report;
use serde::Deserialize;

use crate::pane::layout::PaneLayout;
use crate::pane::layout::PaneRegion;
use crate::state::Pane;
use crate::state::PaneAttentionState;
use crate::state::PaneState;
use crate::state::PaneTree;
use crate::state::Tab;

const INITIAL_PANE_ID: u32 = 1;
const INITIAL_TAB_ID: u32 = 1;
const INITIAL_TAB_TITLE: &str = "default";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PaneSnapshotFieldsEntry {
    cmd_label: Option<String>,
    terminal_title: Option<String>,
    tracked_process_state: TrackedProcessState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaneSnapshotFields {
    panes: BTreeMap<PaneId, PaneSnapshotFieldsEntry>,
}

impl PaneSnapshotFields {
    pub fn set_cmd_label(&mut self, pane_id: PaneId, cmd_label: Option<String>) {
        self.panes.entry(pane_id).or_default().cmd_label = cmd_label;
    }

    pub fn set_terminal_title(&mut self, pane_id: PaneId, terminal_title: Option<String>) {
        self.panes.entry(pane_id).or_default().terminal_title = terminal_title;
    }

    pub fn set_tracked_process_state(&mut self, pane_id: PaneId, tracked_process_state: TrackedProcessState) {
        self.panes.entry(pane_id).or_default().tracked_process_state = tracked_process_state;
    }

    pub fn cmd_label(&self, pane_id: PaneId) -> Option<&str> {
        self.panes.get(&pane_id)?.cmd_label.as_deref()
    }

    pub fn terminal_title(&self, pane_id: PaneId) -> Option<&str> {
        self.panes.get(&pane_id)?.terminal_title.as_deref()
    }

    pub fn tracked_process_state(&self, pane_id: PaneId) -> TrackedProcessState {
        self.panes
            .get(&pane_id)
            .map_or(TrackedProcessState::None, |pane| pane.tracked_process_state)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionMetadata {
    pub cmd_label: String,
    pub cwd: String,
    pub started_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SessionLayout {
    pub active_tab: TabId,
    pub entries: Vec<Tab>,
    pub session: SessionName,
}

/// Active pane at the instant it was read from a `SessionLayout`.
///
/// Use this before mutating focus/layout; it is a focused-pane proof for one input turn, not a durable pane handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivePaneId(PaneId);

impl ActivePaneId {
    pub const fn pane_id(self) -> PaneId {
        self.0
    }
}

impl SessionLayout {
    pub fn initial(session: &SessionName, metadata: SessionMetadata) -> rootcause::Result<Self> {
        let pane_id = PaneId::new(INITIAL_PANE_ID)?;
        let tab_id = TabId::new(INITIAL_TAB_ID)?;

        Ok(Self {
            active_tab: tab_id,
            entries: vec![Tab {
                active_pane: pane_id,
                id: tab_id,
                pane_tree: PaneTree::Pane(Pane {
                    attention_state: PaneAttentionState::Idle,
                    cmd_label: metadata.cmd_label.clone(),
                    cwd: metadata.cwd,
                    focus_seq: 1,
                    id: pane_id,
                    started_at: metadata.started_at,
                    state: PaneState::Running,
                    title: metadata.cmd_label,
                }),
                title: INITIAL_TAB_TITLE.to_owned(),
            }],
            session: session.clone(),
        })
    }

    pub fn snapshot(&self) -> rootcause::Result<LayoutSnapshot> {
        self.snapshot_with_terminal_titles(&[])
    }

    pub fn snapshot_with_terminal_titles(
        &self,
        terminal_titles: &[(PaneId, Option<String>)],
    ) -> rootcause::Result<LayoutSnapshot> {
        let mut snapshot_fields = PaneSnapshotFields::default();
        for (pane_id, terminal_title) in terminal_titles {
            snapshot_fields.set_terminal_title(*pane_id, terminal_title.clone());
        }
        self.snapshot_with_runtime_metadata(&snapshot_fields)
    }

    pub fn snapshot_with_runtime_metadata(
        &self,
        snapshot_fields: &PaneSnapshotFields,
    ) -> rootcause::Result<LayoutSnapshot> {
        let tabs = self
            .entries
            .iter()
            .map(|tab| tab.snapshot_with_runtime_metadata(snapshot_fields))
            .collect::<rootcause::Result<Vec<_>>>()?;
        LayoutSnapshot::new(self.active_tab, tabs)
    }

    pub fn active_tab_index(&self) -> rootcause::Result<usize> {
        self.entries
            .iter()
            .position(|tab| tab.id == self.active_tab)
            .ok_or_else(|| {
                report!("muxr active tab is missing from server layout")
                    .attach(format!("active_tab={}", self.active_tab))
            })
    }

    pub fn active_tab(&self) -> rootcause::Result<&Tab> {
        self.entries
            .iter()
            .find(|tab| tab.id == self.active_tab)
            .ok_or_else(|| {
                report!("muxr active tab is missing from server layout")
                    .attach(format!("active_tab={}", self.active_tab))
            })
    }

    pub fn active_tab_mut(&mut self) -> rootcause::Result<&mut Tab> {
        let active_tab = self.active_tab;
        self.entries.iter_mut().find(|tab| tab.id == active_tab).ok_or_else(|| {
            report!("muxr active tab is missing from server layout").attach(format!("active_tab={active_tab}"))
        })
    }

    pub fn active_pane_id(&self) -> rootcause::Result<PaneId> {
        Ok(self.active_tab()?.active_pane)
    }

    /// Mint a point-in-time active-pane token for callers that must prove user input targeted the focused pane.
    pub fn active_pane_token(&self) -> rootcause::Result<ActivePaneId> {
        Ok(ActivePaneId(self.active_pane_id()?))
    }

    /// Return panes in layout order.
    pub fn panes(&self) -> Vec<&Pane> {
        self.entries.iter().flat_map(Tab::panes).collect()
    }

    /// Return pane ids in layout order.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        for tab in &self.entries {
            tab.pane_tree.append_pane_ids(&mut ids);
        }
        ids
    }

    /// Visit pane ids in layout order.
    pub fn for_each_pane_id(&self, mut visit: impl FnMut(PaneId)) {
        for tab in &self.entries {
            self::for_each_pane_tree_id(&tab.pane_tree, &mut visit);
        }
    }

    /// Find one pane by id.
    pub fn pane(&self, pane_id: PaneId) -> Option<&Pane> {
        self.entries.iter().flat_map(Tab::panes).find(|pane| pane.id == pane_id)
    }

    /// Find one mutable pane by id.
    pub fn pane_mut(&mut self, pane_id: PaneId) -> Option<&mut Pane> {
        self.entries.iter_mut().find_map(|tab| tab.pane_tree.pane_mut(pane_id))
    }

    /// Apply path-like terminal titles to pane cwd metadata.
    pub fn sync_terminal_titles(&mut self, terminal_titles: &[(PaneId, Option<String>)]) -> bool {
        let mut changed = false;
        for (pane_id, title) in terminal_titles {
            if let Some(pane) = self.pane_mut(*pane_id) {
                changed |= pane.sync_terminal_title(title.as_deref());
            }
        }
        changed
    }

    pub fn pane_regions(&self, size: &TerminalSize) -> rootcause::Result<Vec<PaneRegion>> {
        Ok(self.pane_layout(size)?.regions().to_vec())
    }

    pub fn pane_layout(&self, size: &TerminalSize) -> rootcause::Result<PaneLayout> {
        self.active_tab()?.pane_layout(size)
    }

    pub fn pane_tab_index(&self, pane_id: PaneId) -> rootcause::Result<usize> {
        for (tab_index, tab) in self.entries.iter().enumerate() {
            if tab.contains_pane(pane_id) {
                return Ok(tab_index);
            }
        }

        Err(report!("muxr pane is missing from server layout").attach(format!("pane_id={pane_id}")))
    }

    pub fn remove_tab_at(&mut self, tab_index: usize) -> rootcause::Result<()> {
        self.entries.remove(tab_index);
        if !self.entries.iter().any(|tab| tab.id == self.active_tab) {
            let next_tab_index = if tab_index >= self.entries.len() {
                tab_index.saturating_sub(1)
            } else {
                tab_index
            };
            let next_tab = self
                .entries
                .get(next_tab_index)
                .ok_or_else(|| report!("muxr next tab is missing after pane removal"))?;
            self.active_tab = next_tab.id;
        }
        Ok(())
    }

    pub fn next_tab_number(&self) -> rootcause::Result<u32> {
        self::next_number(self.entries.iter().map(|tab| tab.id.get()), "tab")
    }

    pub fn next_pane_number(&self) -> rootcause::Result<u32> {
        self::next_number(self.entries.iter().flat_map(Tab::pane_ids).map(PaneId::get), "pane")
    }
}

fn next_number(ids: impl Iterator<Item = u32>, kind: &str) -> rootcause::Result<u32> {
    let max_number = ids.max().unwrap_or(0);

    max_number
        .checked_add(1)
        .ok_or_else(|| report!("muxr layout id counter overflowed").attach(format!("kind={kind}")))
}

fn for_each_pane_tree_id(tree: &PaneTree, visit: &mut impl FnMut(PaneId)) {
    match tree {
        PaneTree::Pane(pane) => visit(pane.id),
        PaneTree::Split { first, second, .. } => {
            self::for_each_pane_tree_id(first, visit);
            self::for_each_pane_tree_id(second, visit);
        }
    }
}

#[cfg(test)]
pub mod test_helpers {
    use muxr_core::TerminalSize;

    use super::*;
    use crate::pane::borders::PaneBorderAxis;
    use crate::pane::split::PaneSplitRatio;

    const BALANCED_TEST_SPLIT_RATIO: u16 = 500;

    pub type PaneRegionTuple = (String, u16, u16, u16, u16);

    pub fn layout(raw: &str) -> rootcause::Result<SessionLayout> {
        let session: SessionName = raw.parse()?;
        SessionLayout::initial(&session, metadata("sh", 1))
    }

    pub fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }

    pub fn force_balanced_test_split_ratio(layout: &mut SessionLayout) -> rootcause::Result<()> {
        let ratio = PaneSplitRatio::new(BALANCED_TEST_SPLIT_RATIO)?;
        for tab in &mut layout.entries {
            self::force_balanced_pane_tree_split_ratio(&mut tab.pane_tree, ratio);
        }
        Ok(())
    }

    pub fn layout_tab_ids(layout: &SessionLayout) -> rootcause::Result<Vec<String>> {
        Ok(layout
            .snapshot()?
            .tabs()
            .iter()
            .map(|tab| tab.id().to_string())
            .collect::<Vec<_>>())
    }

    pub fn layout_active_tab_pane_ids(layout: &SessionLayout) -> rootcause::Result<Vec<String>> {
        let snapshot = layout.snapshot()?;
        let active_tab = snapshot
            .tabs()
            .iter()
            .find(|tab| tab.id() == snapshot.active_tab())
            .ok_or_else(|| report!("expected active tab in muxr test layout snapshot"))?;

        Ok(active_tab.panes().iter().map(|pane| pane.id.to_string()).collect())
    }

    pub fn layout_active_tab_pane_regions(
        layout: &SessionLayout,
        size: &TerminalSize,
    ) -> rootcause::Result<Vec<PaneRegionTuple>> {
        Ok(layout
            .pane_regions(size)?
            .iter()
            .map(|region| {
                (
                    region.id.to_string(),
                    region.area.origin.col,
                    region.area.origin.row,
                    region.area.size.cols,
                    region.area.size.rows,
                )
            })
            .collect())
    }

    pub fn layout_active_tab_pane_borders(
        layout: &SessionLayout,
        size: &TerminalSize,
    ) -> rootcause::Result<Vec<(PaneBorderAxis, u16, u16, u16)>> {
        Ok(layout
            .pane_layout(size)?
            .borders()
            .iter()
            .map(|border| (border.axis(), border.col(), border.row(), border.len()))
            .collect())
    }

    fn force_balanced_pane_tree_split_ratio(pane_tree: &mut PaneTree, ratio: PaneSplitRatio) {
        match pane_tree {
            PaneTree::Pane(_) => {}
            PaneTree::Split {
                first_ratio,
                first,
                second,
                ..
            } => {
                *first_ratio = ratio;
                self::force_balanced_pane_tree_split_ratio(first, ratio);
                self::force_balanced_pane_tree_split_ratio(second, ratio);
            }
        }
    }
}
