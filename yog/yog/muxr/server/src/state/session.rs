use muxr_core::ClientMousePosition;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::SessionName;
use muxr_core::TabId;
use muxr_core::TerminalSize;
use rootcause::report;
use serde::Deserialize;

use crate::geometry::PaneLayout;
use crate::geometry::PaneRegion;
use crate::state::Pane;
use crate::state::PaneNode;
use crate::state::Tab;

const INITIAL_PANE_ID: &str = "pane-1";
const INITIAL_TAB_ID: &str = "tab-1";
const INITIAL_TAB_TITLE: &str = "default";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionMetadata {
    pub command_label: String,
    pub cwd: String,
    pub started_at: u64,
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
pub struct SessionLayout {
    pub active_tab: TabId,
    pub entries: Vec<Tab>,
    pub session: SessionName,
}

impl SessionLayout {
    pub fn initial(session: &SessionName, metadata: SessionMetadata) -> rootcause::Result<Self> {
        let pane_id = PaneId::new(INITIAL_PANE_ID)?;
        let tab_id = TabId::new(INITIAL_TAB_ID)?;

        Ok(Self {
            active_tab: tab_id.clone(),
            entries: vec![Tab {
                active_pane: pane_id.clone(),
                id: tab_id,
                pane_tree: PaneNode::leaf(Pane::new(
                    pane_id,
                    metadata.command_label,
                    metadata.cwd,
                    metadata.started_at,
                    1,
                )),
                title: INITIAL_TAB_TITLE.to_owned(),
            }],
            session: session.clone(),
        })
    }

    pub fn snapshot(&self) -> rootcause::Result<LayoutSnapshot> {
        let tabs = self
            .entries
            .iter()
            .map(Tab::snapshot)
            .collect::<rootcause::Result<Vec<_>>>()?;
        LayoutSnapshot::new(self.active_tab.clone(), tabs)
    }

    pub fn pane_at(&self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<Option<PaneId>> {
        self.active_tab()?.pane_at(size, position)
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
        let active_tab = self.active_tab.clone();
        self.entries.iter_mut().find(|tab| tab.id == active_tab).ok_or_else(|| {
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
        self.entries
            .iter()
            .flat_map(Tab::panes)
            .map(|pane| pane.id().clone())
            .collect()
    }

    pub fn pane_regions(&self, size: &TerminalSize) -> rootcause::Result<Vec<PaneRegion>> {
        Ok(self.pane_layout(size)?.regions().to_vec())
    }

    pub fn pane_layout(&self, size: &TerminalSize) -> rootcause::Result<PaneLayout> {
        self.active_tab()?.pane_layout(size)
    }

    pub fn pane_tab_index(&self, pane_id: &PaneId) -> rootcause::Result<usize> {
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
            self.active_tab = next_tab.id.clone();
        }
        Ok(())
    }

    pub fn next_tab_number(&self) -> rootcause::Result<u64> {
        self::next_number(self.entries.iter().map(|tab| tab.id.as_ref()), "tab-")
    }

    pub fn next_pane_number(&self) -> rootcause::Result<u64> {
        self::next_number(self.entries.iter().flat_map(Tab::pane_ids), "pane-")
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
