use muxr_core::ClientMousePosition;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::PaneSnapshot;
use muxr_core::SessionName;
use muxr_core::TabId;
use muxr_core::TerminalSize;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

use crate::layout::region::PaneLayout;
use crate::layout::region::PaneRegion;
use crate::pty::PtyExitStatus;

mod pane_tree;
pub mod persisted;
pub mod region;
mod split;
mod tabs;

const INITIAL_PANE_ID: &str = "pane-1";
const INITIAL_TAB_ID: &str = "tab-1";
const INITIAL_TAB_TITLE: &str = "default";
pub const VERSION: u16 = 4;
const SPLIT_RATIO_SCALE: u16 = 1000;
const SPLIT_RATIO_HALF_SCALE: u16 = 500;
const DEFAULT_SPLIT_RATIO: u16 = 500;
const MIN_SPLIT_RATIO: u16 = 50;
const MAX_SPLIT_RATIO: u16 = 950;
const SPLIT_RESIZE_STEP: u16 = 50;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionMetadata {
    command_label: String,
    cwd: String,
    started_at: u64,
}

impl SessionMetadata {
    pub const fn new(command_label: String, cwd: String, started_at: u64) -> Self {
        Self {
            command_label,
            cwd,
            started_at,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Layout {
    active_tab: TabId,
    session: SessionName,
    tabs: Vec<Tab>,
}

impl Layout {
    pub fn initial(session: &SessionName, metadata: SessionMetadata) -> rootcause::Result<Self> {
        let pane_id = PaneId::new(INITIAL_PANE_ID)?;
        let tab_id = TabId::new(INITIAL_TAB_ID)?;

        Ok(Self {
            active_tab: tab_id.clone(),
            session: session.clone(),
            tabs: vec![Tab {
                active_pane: pane_id.clone(),
                id: tab_id,
                pane_tree: PaneNode::leaf(Pane {
                    command_label: metadata.command_label.clone(),
                    cwd: metadata.cwd,
                    exit_status: None,
                    exited_at: None,
                    focus_seq: 1,
                    id: pane_id,
                    started_at: metadata.started_at,
                    title: metadata.command_label,
                }),
                title: INITIAL_TAB_TITLE.to_owned(),
            }],
        })
    }

    pub fn snapshot(&self) -> rootcause::Result<LayoutSnapshot> {
        let tabs = self
            .tabs
            .iter()
            .map(Tab::snapshot)
            .collect::<rootcause::Result<Vec<_>>>()?;
        LayoutSnapshot::new(self.active_tab.clone(), tabs)
    }

    pub fn create_tab(&mut self, metadata: SessionMetadata) -> rootcause::Result<PaneId> {
        let tab_index = self.active_tab_index()?;
        let tab_number = self.next_tab_number()?;
        let pane_number = self.next_pane_number()?;
        let tab_id = TabId::new(format!("tab-{tab_number}"))?;
        let pane_id = PaneId::new(format!("pane-{pane_number}"))?;
        let insert_index = tab_index
            .checked_add(1)
            .ok_or_else(|| report!("muxr tab insert index overflowed"))?;

        self.tabs.insert(
            insert_index,
            Tab {
                active_pane: pane_id.clone(),
                id: tab_id.clone(),
                pane_tree: PaneNode::leaf(Pane {
                    command_label: metadata.command_label.clone(),
                    cwd: metadata.cwd,
                    exit_status: None,
                    exited_at: None,
                    focus_seq: 1,
                    id: pane_id.clone(),
                    started_at: metadata.started_at,
                    title: metadata.command_label,
                }),
                title: format!("tab {tab_number}"),
            },
        );
        self.active_tab = tab_id;
        Ok(pane_id)
    }

    pub fn split_active_pane(
        &mut self,
        metadata: SessionMetadata,
        split_axis: PaneSplitAxis,
    ) -> rootcause::Result<PaneId> {
        let pane_number = self.next_pane_number()?;
        let pane_id = PaneId::new(format!("pane-{pane_number}"))?;
        let tab = self.active_tab_mut()?;
        let focus_seq = tab.next_focus_seq()?;
        let new_pane = Pane {
            command_label: metadata.command_label.clone(),
            cwd: metadata.cwd,
            exit_status: None,
            exited_at: None,
            focus_seq,
            id: pane_id.clone(),
            started_at: metadata.started_at,
            title: metadata.command_label,
        };
        tab.split_active_pane(&new_pane, split_axis)?;
        tab.active_pane = pane_id.clone();
        Ok(pane_id)
    }

    pub fn resize_active_pane(&mut self, direction: PaneResizeDirection) -> rootcause::Result<bool> {
        self.active_tab_mut()?.resize_active_pane(direction)
    }

    pub fn focus_pane_at(&mut self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<bool> {
        self.active_tab_mut()?.focus_pane_at(size, position)
    }

    pub fn focus_pane_direction(
        &mut self,
        size: &TerminalSize,
        direction: PaneFocusDirection,
    ) -> rootcause::Result<bool> {
        self.active_tab_mut()?.focus_pane_direction(size, direction)
    }

    pub fn pane_at(&self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<Option<PaneId>> {
        self.active_tab()?.pane_at(size, position)
    }

    pub fn close_active_pane(&mut self, exited_at: u64) -> rootcause::Result<ClosePaneOutcome> {
        let active_tab_index = self.active_tab_index()?;
        let final_pane = self.tabs.len() == 1
            && self
                .tabs
                .get(active_tab_index)
                .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
                .pane_count()
                == 1;
        let active_pane = self
            .tabs
            .get(active_tab_index)
            .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
            .active_pane
            .clone();

        if final_pane {
            self.tabs
                .get_mut(active_tab_index)
                .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
                .mark_pane_exited(&active_pane, exited_at, None)?;
            return Ok(ClosePaneOutcome::Final { pane_id: active_pane });
        }

        if self
            .tabs
            .get(active_tab_index)
            .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
            .pane_count()
            == 1
        {
            self.remove_tab_at(active_tab_index)?;
        } else {
            let tab = self
                .tabs
                .get_mut(active_tab_index)
                .ok_or_else(|| report!("muxr active tab index is outside server layout"))?;
            let fallback_pane = tab.remove_pane(&active_pane)?;
            let _focused = tab.focus_pane(fallback_pane)?;
        }

        Ok(ClosePaneOutcome::Removed { pane_id: active_pane })
    }

    pub fn remove_exited_pane(
        &mut self,
        pane_id: &PaneId,
        exited_at: u64,
        exit_status: Option<PtyExitStatus>,
    ) -> rootcause::Result<PaneExitOutcome> {
        let tab_index = self.pane_tab_index(pane_id)?;

        if self.tabs.len() == 1
            && self
                .tabs
                .get(tab_index)
                .ok_or_else(|| report!("muxr exited pane tab is missing"))?
                .pane_count()
                == 1
        {
            let tab = self
                .tabs
                .get_mut(tab_index)
                .ok_or_else(|| report!("muxr final pane tab is missing"))?;
            tab.mark_pane_exited(pane_id, exited_at, exit_status)?;
            return Ok(PaneExitOutcome::Final);
        }

        if self
            .tabs
            .get(tab_index)
            .ok_or_else(|| report!("muxr exited pane tab is missing"))?
            .pane_count()
            == 1
        {
            self.remove_tab_at(tab_index)?;
            return Ok(PaneExitOutcome::Removed);
        }

        let tab = self
            .tabs
            .get_mut(tab_index)
            .ok_or_else(|| report!("muxr exited pane tab is missing"))?;
        let removed_active_pane = tab.active_pane == *pane_id;
        let fallback_pane = tab.remove_pane(pane_id)?;
        if removed_active_pane {
            let _focused = tab.focus_pane(fallback_pane)?;
        }
        Ok(PaneExitOutcome::Removed)
    }

    pub fn focus_previous_tab(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        let previous_index = if tab_index == 0 {
            self.tabs.len().saturating_sub(1)
        } else {
            tab_index.saturating_sub(1)
        };
        self.active_tab = self
            .tabs
            .get(previous_index)
            .ok_or_else(|| report!("muxr previous tab is missing from server layout"))?
            .id
            .clone();
        Ok(())
    }

    pub fn focus_next_tab(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        let next_index = tab_index
            .checked_add(1)
            .filter(|index| *index < self.tabs.len())
            .unwrap_or(0);
        self.active_tab = self
            .tabs
            .get(next_index)
            .ok_or_else(|| report!("muxr next tab is missing from server layout"))?
            .id
            .clone();
        Ok(())
    }

    pub fn move_active_tab_previous(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        if tab_index > 0 {
            self.tabs.swap(tab_index, tab_index.saturating_sub(1));
        }
        Ok(())
    }

    pub fn move_active_tab_next(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        let Some(next_index) = tab_index.checked_add(1) else {
            return Err(report!("muxr next tab index overflowed"));
        };
        if next_index < self.tabs.len() {
            self.tabs.swap(tab_index, next_index);
        }
        Ok(())
    }

    fn active_tab_index(&self) -> rootcause::Result<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.id == self.active_tab)
            .ok_or_else(|| {
                report!("muxr active tab is missing from server layout")
                    .attach(format!("active_tab={}", self.active_tab))
            })
    }

    fn active_tab(&self) -> rootcause::Result<&Tab> {
        self.tabs.iter().find(|tab| tab.id == self.active_tab).ok_or_else(|| {
            report!("muxr active tab is missing from server layout").attach(format!("active_tab={}", self.active_tab))
        })
    }

    fn active_tab_mut(&mut self) -> rootcause::Result<&mut Tab> {
        let active_tab = self.active_tab.clone();
        self.tabs.iter_mut().find(|tab| tab.id == active_tab).ok_or_else(|| {
            report!("muxr active tab is missing from server layout").attach(format!("active_tab={active_tab}"))
        })
    }

    pub fn active_pane_id(&self) -> rootcause::Result<PaneId> {
        Ok(self.active_tab()?.active_pane.clone())
    }

    #[cfg(test)]
    pub const fn active_tab_id(&self) -> &TabId {
        &self.active_tab
    }

    pub fn pane_ids(&self) -> Vec<PaneId> {
        self.tabs
            .iter()
            .flat_map(Tab::panes)
            .map(|pane| pane.id.clone())
            .collect()
    }

    pub fn pane_regions(&self, size: &TerminalSize) -> rootcause::Result<Vec<PaneRegion>> {
        Ok(self.pane_layout(size)?.regions().to_vec())
    }

    pub fn pane_layout(&self, size: &TerminalSize) -> rootcause::Result<PaneLayout> {
        self.active_tab()?.pane_layout(size)
    }

    fn pane_tab_index(&self, pane_id: &PaneId) -> rootcause::Result<usize> {
        for (tab_index, tab) in self.tabs.iter().enumerate() {
            if tab.contains_pane(pane_id) {
                return Ok(tab_index);
            }
        }

        Err(report!("muxr pane is missing from server layout").attach(format!("pane_id={pane_id}")))
    }

    fn remove_tab_at(&mut self, tab_index: usize) -> rootcause::Result<()> {
        self.tabs.remove(tab_index);
        if !self.tabs.iter().any(|tab| tab.id == self.active_tab) {
            let next_tab_index = if tab_index >= self.tabs.len() {
                tab_index.saturating_sub(1)
            } else {
                tab_index
            };
            let next_tab = self
                .tabs
                .get(next_tab_index)
                .ok_or_else(|| report!("muxr next tab is missing after pane removal"))?;
            self.active_tab = next_tab.id.clone();
        }
        Ok(())
    }

    fn next_tab_number(&self) -> rootcause::Result<u64> {
        self::next_number(self.tabs.iter().map(|tab| tab.id.as_ref()), "tab-")
    }

    fn next_pane_number(&self) -> rootcause::Result<u64> {
        self::next_number(self.tabs.iter().flat_map(Tab::pane_ids), "pane-")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PaneSplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct PaneSplitRatio(u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneSplitResize {
    DecreaseFirst,
    IncreaseFirst,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClosePaneOutcome {
    Final { pane_id: PaneId },
    Removed { pane_id: PaneId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneExitOutcome {
    Final,
    Removed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneFocusDirection {
    Down,
    Left,
    Right,
    Up,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneResizeDirection {
    Down,
    Left,
    Right,
    Up,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Tab {
    active_pane: PaneId,
    id: TabId,
    pane_tree: PaneNode,
    title: String,
}

// Pane splits are a tree so a new split mutates only the active leaf; a tab-wide axis would reflow siblings.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PaneNode {
    Leaf {
        pane: Pane,
    },
    Split {
        axis: PaneSplitAxis,
        first_ratio: PaneSplitRatio,
        first: Box<Self>,
        second: Box<Self>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Pane {
    command_label: String,
    cwd: String,
    exit_status: Option<PtyExitStatus>,
    exited_at: Option<u64>,
    focus_seq: u64,
    id: PaneId,
    started_at: u64,
    title: String,
}

impl Pane {
    pub fn snapshot(&self) -> PaneSnapshot {
        PaneSnapshot::new(self.id.clone(), self.title.clone())
    }
}

fn next_number<'a>(ids: impl Iterator<Item = &'a str>, prefix: &str) -> rootcause::Result<u64> {
    let max_number = ids
        .filter_map(|id| id.strip_prefix(prefix))
        .filter_map(|suffix| suffix.parse::<u64>().ok())
        .max()
        .unwrap_or(0);

    max_number
        .checked_add(1)
        .ok_or_else(|| report!("muxr layout id counter overflowed").attach(format!("prefix={prefix}")))
}
