use std::collections::BTreeSet;
use std::fmt;
use std::num::NonZeroU32;

use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

use super::ClientMousePosition;
use super::PaneAgentState;

#[derive(
    rkyv::Archive,
    Clone,
    Copy,
    Debug,
    Deserialize,
    rkyv::Deserialize,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    rkyv::Serialize,
)]
#[serde(transparent)]
pub struct TabId(NonZeroU32);

impl TabId {
    /// Build a tab id for layout snapshots and persisted session state.
    ///
    /// # Errors
    /// - The id is zero.
    pub fn new(id: u32) -> rootcause::Result<Self> {
        let Some(id) = NonZeroU32::new(id) else {
            return Err(report!("invalid muxr tab id").attach("id=0"));
        };
        Ok(Self(id))
    }

    /// Return the numeric tab id.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tab-{}", self.get())
    }
}

#[derive(
    rkyv::Archive,
    Clone,
    Copy,
    Debug,
    Deserialize,
    rkyv::Deserialize,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    rkyv::Serialize,
)]
#[serde(transparent)]
pub struct PaneId(NonZeroU32);

impl PaneId {
    /// Build a pane id for layout snapshots and persisted session state.
    ///
    /// # Errors
    /// - The id is zero.
    pub fn new(id: u32) -> rootcause::Result<Self> {
        let Some(id) = NonZeroU32::new(id) else {
            return Err(report!("invalid muxr pane id").attach("id=0"));
        };
        Ok(Self(id))
    }

    /// Return the numeric pane id.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pane-{}", self.get())
    }
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct LayoutSnapshot {
    active_tab: TabId,
    tabs: Vec<TabSnapshot>,
}

impl LayoutSnapshot {
    /// Build a layout snapshot with active tab and pane invariants checked.
    ///
    /// # Errors
    /// - Any tab or pane id is duplicated.
    /// - The layout has no tabs.
    /// - The active tab does not exist.
    /// - Any tab has no panes or an active pane that does not exist.
    pub fn new(active_tab: TabId, tabs: Vec<TabSnapshot>) -> rootcause::Result<Self> {
        let snapshot = Self { active_tab, tabs };
        snapshot.validate()?;
        Ok(snapshot)
    }

    #[must_use]
    pub const fn active_tab(&self) -> &TabId {
        &self.active_tab
    }

    #[must_use]
    pub fn tabs(&self) -> &[TabSnapshot] {
        &self.tabs
    }

    fn validate(&self) -> rootcause::Result<()> {
        if self.tabs.is_empty() {
            return Err(report!("invalid muxr layout snapshot").attach("reason=tabs must not be empty"));
        }
        if !self.tabs.iter().any(|tab| tab.id == self.active_tab) {
            return Err(report!("invalid muxr layout snapshot")
                .attach("reason=active tab is missing")
                .attach(format!("active_tab={}", self.active_tab)));
        }

        let mut seen_tab_ids = BTreeSet::new();
        let mut seen_pane_ids = BTreeSet::new();
        for tab in &self.tabs {
            tab.validate()?;
            if !seen_tab_ids.insert(tab.id) {
                return Err(report!("invalid muxr layout snapshot")
                    .attach("reason=duplicate tab id")
                    .attach(format!("tab_id={}", tab.id)));
            }

            for pane in &tab.panes {
                if !seen_pane_ids.insert(pane.id) {
                    return Err(report!("invalid muxr layout snapshot")
                        .attach("reason=duplicate pane id")
                        .attach(format!("tab_id={}", tab.id))
                        .attach(format!("pane_id={}", pane.id)));
                }
            }
        }

        Ok(())
    }
}

impl<D> rkyv::Deserialize<LayoutSnapshot, D> for ArchivedLayoutSnapshot
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<LayoutSnapshot, D::Error> {
        let active_tab = rkyv::Deserialize::<TabId, D>::deserialize(&self.active_tab, deserializer)?;
        let tabs = rkyv::Deserialize::<Vec<TabSnapshot>, D>::deserialize(&self.tabs, deserializer)?;
        LayoutSnapshot::new(active_tab, tabs).map_err(super::rkyv_deserialize_error::<D::Error>)
    }
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct TabSnapshot {
    active_pane: PaneId,
    id: TabId,
    panes: Vec<PaneSnapshot>,
    title: String,
}

impl TabSnapshot {
    /// Build a tab snapshot with active pane invariants checked.
    ///
    /// # Errors
    /// - Any pane id is duplicated.
    /// - The tab has no panes.
    /// - The active pane does not exist.
    pub fn new(
        id: TabId,
        title: impl Into<String>,
        active_pane: PaneId,
        panes: Vec<PaneSnapshot>,
    ) -> rootcause::Result<Self> {
        let snapshot = Self {
            active_pane,
            id,
            panes,
            title: title.into(),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    #[must_use]
    pub const fn active_pane(&self) -> &PaneId {
        &self.active_pane
    }

    #[must_use]
    pub const fn id(&self) -> &TabId {
        &self.id
    }

    #[must_use]
    pub fn panes(&self) -> &[PaneSnapshot] {
        &self.panes
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    fn validate(&self) -> rootcause::Result<()> {
        if self.panes.is_empty() {
            return Err(report!("invalid muxr tab snapshot")
                .attach("reason=panes must not be empty")
                .attach(format!("tab_id={}", self.id)));
        }
        if !self.panes.iter().any(|pane| pane.id == self.active_pane) {
            return Err(report!("invalid muxr tab snapshot")
                .attach("reason=active pane is missing")
                .attach(format!("tab_id={}", self.id))
                .attach(format!("active_pane={}", self.active_pane)));
        }

        let mut seen_pane_ids = BTreeSet::new();
        for pane in &self.panes {
            if !seen_pane_ids.insert(pane.id) {
                return Err(report!("invalid muxr tab snapshot")
                    .attach("reason=duplicate pane id")
                    .attach(format!("tab_id={}", self.id))
                    .attach(format!("pane_id={}", pane.id)));
            }
        }

        Ok(())
    }
}

impl<D> rkyv::Deserialize<TabSnapshot, D> for ArchivedTabSnapshot
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<TabSnapshot, D::Error> {
        let active_pane = rkyv::Deserialize::<PaneId, D>::deserialize(&self.active_pane, deserializer)?;
        let id = rkyv::Deserialize::<TabId, D>::deserialize(&self.id, deserializer)?;
        let panes = rkyv::Deserialize::<Vec<PaneSnapshot>, D>::deserialize(&self.panes, deserializer)?;
        let title = rkyv::Deserialize::<String, D>::deserialize(&self.title, deserializer)?;
        TabSnapshot::new(id, title, active_pane, panes).map_err(super::rkyv_deserialize_error::<D::Error>)
    }
}

/// Agent status rendered by the client tab bar.

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct PaneSnapshot {
    /// Agent status used by the client tab bar.
    pub agent_state: PaneAgentState,
    /// Current pane working directory, used by the client tab bar.
    pub cwd: String,
    /// Shell-provided cmd label from the pane terminal title, used by the client tab bar.
    pub cmd_label: Option<String>,
    /// Stable pane id.
    pub id: PaneId,
    /// Pane title displayed in tab and pane UI.
    pub title: String,
}

impl<D> rkyv::Deserialize<PaneSnapshot, D> for ArchivedPaneSnapshot
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<PaneSnapshot, D::Error> {
        let agent_state = rkyv::Deserialize::<PaneAgentState, D>::deserialize(&self.agent_state, deserializer)?;
        let cwd = rkyv::Deserialize::<String, D>::deserialize(&self.cwd, deserializer)?;
        let cmd_label = rkyv::Deserialize::<Option<String>, D>::deserialize(&self.cmd_label, deserializer)?;
        let id = rkyv::Deserialize::<PaneId, D>::deserialize(&self.id, deserializer)?;
        let title = rkyv::Deserialize::<String, D>::deserialize(&self.title, deserializer)?;
        Ok(PaneSnapshot {
            agent_state,
            cwd,
            cmd_label,
            id,
            title,
        })
    }
}

/// Mouse tracking mode requested by the application running in a pane.
#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum PaneMouseMode {
    AnyMotion,
    ButtonMotion,
    None,
    Press,
    PressRelease,
}

impl PaneMouseMode {
    /// Return whether the pane application requested any terminal mouse tracking mode.
    #[must_use]
    pub const fn tracking_enabled(self) -> bool {
        !matches!(self, Self::None)
    }

    /// Return whether the outer terminal must report motion even when no button is pressed.
    #[must_use]
    pub const fn needs_any_motion_capture(self) -> bool {
        matches!(self, Self::AnyMotion)
    }
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct PaneRegionSnapshot {
    id: PaneId,
    col: u16,
    row: u16,
    cols: u16,
    mouse_mode: PaneMouseMode,
    rows: u16,
    visible_top_row: u64,
}

impl PaneRegionSnapshot {
    /// Build a visible pane region for the current rendered frame.
    ///
    /// # Errors
    /// - The region has zero columns or rows.
    /// - The region row or column range overflows.
    pub fn new(
        id: PaneId,
        col: u16,
        row: u16,
        cols: u16,
        rows: u16,
        mouse_mode: PaneMouseMode,
        visible_top_row: u64,
    ) -> rootcause::Result<Self> {
        let region = Self {
            id,
            col,
            row,
            cols,
            mouse_mode,
            rows,
            visible_top_row,
        };
        region.validate()?;
        Ok(region)
    }

    #[must_use]
    pub const fn col(&self) -> u16 {
        self.col
    }

    #[must_use]
    pub const fn cols(&self) -> u16 {
        self.cols
    }

    #[must_use]
    pub const fn id(&self) -> &PaneId {
        &self.id
    }

    /// Return the pane application's current terminal mouse tracking mode.
    #[must_use]
    pub const fn mouse_mode(&self) -> PaneMouseMode {
        self.mouse_mode
    }

    /// Return whether the pane application requested terminal mouse tracking.
    #[must_use]
    pub const fn mouse_tracking_enabled(&self) -> bool {
        self.mouse_mode.tracking_enabled()
    }

    #[must_use]
    pub const fn row(&self) -> u16 {
        self.row
    }

    #[must_use]
    pub const fn rows(&self) -> u16 {
        self.rows
    }

    /// Return the stable content row rendered at the top of this pane's visible viewport.
    #[must_use]
    pub const fn visible_top_row(&self) -> u64 {
        self.visible_top_row
    }

    #[must_use]
    pub const fn contains(&self, row: u16, col: u16) -> bool {
        let Some(end_row) = self.row.checked_add(self.rows) else {
            return false;
        };
        let Some(end_col) = self.col.checked_add(self.cols) else {
            return false;
        };

        row >= self.row && row < end_row && col >= self.col && col < end_col
    }

    fn validate(&self) -> rootcause::Result<()> {
        if self.cols == 0 {
            return Err(report!("invalid muxr pane region").attach("reason=cols must be nonzero"));
        }
        if self.rows == 0 {
            return Err(report!("invalid muxr pane region").attach("reason=rows must be nonzero"));
        }
        if self.col.checked_add(self.cols).is_none() {
            return Err(report!("invalid muxr pane region").attach("reason=column range overflowed"));
        }
        if self.row.checked_add(self.rows).is_none() {
            return Err(report!("invalid muxr pane region").attach("reason=row range overflowed"));
        }
        Ok(())
    }
}

impl<D> rkyv::Deserialize<PaneRegionSnapshot, D> for ArchivedPaneRegionSnapshot
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<PaneRegionSnapshot, D::Error> {
        let id = rkyv::Deserialize::<PaneId, D>::deserialize(&self.id, deserializer)?;
        let col = rkyv::Deserialize::<u16, D>::deserialize(&self.col, deserializer)?;
        let row = rkyv::Deserialize::<u16, D>::deserialize(&self.row, deserializer)?;
        let cols = rkyv::Deserialize::<u16, D>::deserialize(&self.cols, deserializer)?;
        let rows = rkyv::Deserialize::<u16, D>::deserialize(&self.rows, deserializer)?;
        let mouse_mode = rkyv::Deserialize::<PaneMouseMode, D>::deserialize(&self.mouse_mode, deserializer)?;
        let visible_top_row = rkyv::Deserialize::<u64, D>::deserialize(&self.visible_top_row, deserializer)?;
        PaneRegionSnapshot::new(id, col, row, cols, rows, mouse_mode, visible_top_row)
            .map_err(super::rkyv_deserialize_error::<D::Error>)
    }
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct PaneRegionsSnapshot {
    regions: Vec<PaneRegionSnapshot>,
}

impl PaneRegionsSnapshot {
    /// Build the pane regions for the currently rendered tab.
    ///
    /// # Errors
    /// - The region list is empty.
    /// - Any region is invalid.
    /// - Any pane id appears more than once.
    pub fn new(regions: Vec<PaneRegionSnapshot>) -> rootcause::Result<Self> {
        let snapshot = Self { regions };
        snapshot.validate()?;
        Ok(snapshot)
    }

    #[must_use]
    pub fn regions(&self) -> &[PaneRegionSnapshot] {
        &self.regions
    }

    #[must_use]
    pub fn pane_at(&self, position: ClientMousePosition) -> Option<&PaneRegionSnapshot> {
        self.regions()
            .iter()
            .find(|region| region.contains(position.row, position.col))
    }

    fn validate(&self) -> rootcause::Result<()> {
        if self.regions.is_empty() {
            return Err(report!("invalid muxr pane regions snapshot").attach("reason=regions must not be empty"));
        }

        let mut seen_pane_ids = BTreeSet::new();
        for region in &self.regions {
            region.validate()?;
            if !seen_pane_ids.insert(region.id) {
                return Err(report!("invalid muxr pane regions snapshot")
                    .attach("reason=duplicate pane id")
                    .attach(format!("pane_id={}", region.id)));
            }
        }

        Ok(())
    }
}

impl<D> rkyv::Deserialize<PaneRegionsSnapshot, D> for ArchivedPaneRegionsSnapshot
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<PaneRegionsSnapshot, D::Error> {
        let regions = rkyv::Deserialize::<Vec<PaneRegionSnapshot>, D>::deserialize(&self.regions, deserializer)?;
        PaneRegionsSnapshot::new(regions).map_err(super::rkyv_deserialize_error::<D::Error>)
    }
}

#[cfg(test)]
pub mod test_helpers {
    use super::*;

    pub const fn raw_layout_snapshot(active_tab: TabId, tabs: Vec<TabSnapshot>) -> LayoutSnapshot {
        LayoutSnapshot { active_tab, tabs }
    }

    pub fn raw_tab_snapshot(
        id: TabId,
        title: impl Into<String>,
        active_pane: PaneId,
        panes: Vec<PaneSnapshot>,
    ) -> TabSnapshot {
        TabSnapshot {
            active_pane,
            id,
            panes,
            title: title.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use rootcause::report;
    use rstest::rstest;

    use super::*;

    #[test]
    fn test_layout_snapshot_single_pane_when_built_returns_stable_layout() -> rootcause::Result<()> {
        let layout = self::layout_snapshot()?;

        pretty_assertions::assert_eq!(layout.active_tab().get(), 1);
        pretty_assertions::assert_eq!(layout.tabs().len(), 1);
        let Some(tab) = layout.tabs().first() else {
            return Err(report!("expected one tab"));
        };
        pretty_assertions::assert_eq!(tab.active_pane().get(), 1);
        pretty_assertions::assert_eq!(tab.panes().len(), 1);
        Ok(())
    }

    #[test]
    fn test_layout_id_new_when_id_is_zero_returns_error() {
        assert2::assert!(TabId::new(0).is_err());
        assert2::assert!(PaneId::new(0).is_err());
    }

    #[test]
    fn test_layout_id_deserialize_when_id_is_zero_returns_error() {
        let raw = "0";

        assert2::assert!(serde_json::from_str::<TabId>(raw).is_err());
        assert2::assert!(serde_json::from_str::<PaneId>(raw).is_err());
    }

    #[test]
    fn test_layout_id_display_when_formatted_returns_human_label() -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(TabId::new(1)?.to_string(), "tab-1");
        pretty_assertions::assert_eq!(PaneId::new(1)?.to_string(), "pane-1");
        Ok(())
    }

    #[rstest]
    #[case::empty_tabs(test_helpers::raw_layout_snapshot(tab_id(1), Vec::new()))]
    #[case::missing_active_tab(test_helpers::raw_layout_snapshot(
            tab_id(99),
            vec![raw_tab_snapshot(1, "default", 1, vec![raw_pane_snapshot(1, "shell")])],
        ))]
    #[case::empty_panes(test_helpers::raw_layout_snapshot(
            tab_id(1),
            vec![test_helpers::raw_tab_snapshot(tab_id(1), "default", pane_id(1), Vec::new())],
        ))]
    #[case::missing_active_pane(test_helpers::raw_layout_snapshot(
            tab_id(1),
            vec![test_helpers::raw_tab_snapshot(
                tab_id(1),
                "default",
                pane_id(99),
                vec![raw_pane_snapshot(1, "shell")]
            )],
        ))]
    #[case::duplicate_tab(test_helpers::raw_layout_snapshot(
            tab_id(1),
            vec![
                raw_tab_snapshot(1, "default", 1, vec![raw_pane_snapshot(1, "shell")]),
                raw_tab_snapshot(1, "other", 2, vec![raw_pane_snapshot(2, "shell")]),
            ],
        ))]
    #[case::duplicate_pane(test_helpers::raw_layout_snapshot(
            tab_id(1),
            vec![test_helpers::raw_tab_snapshot(
                tab_id(1),
                "default",
                pane_id(1),
                vec![raw_pane_snapshot(1, "shell"), raw_pane_snapshot(1, "other")]
            )],
        ))]
    #[case::duplicate_pane_across_tabs(test_helpers::raw_layout_snapshot(
            tab_id(1),
            vec![
                raw_tab_snapshot(1, "default", 1, vec![raw_pane_snapshot(1, "shell")]),
                raw_tab_snapshot(2, "other", 2, vec![
                    raw_pane_snapshot(1, "other"),
                    raw_pane_snapshot(2, "shell"),
                ]),
            ],
        ))]
    fn test_layout_snapshot_validate_when_layout_is_invalid_returns_error(#[case] layout: LayoutSnapshot) {
        assert2::assert!(LayoutSnapshot::new(*layout.active_tab(), layout.tabs().to_vec()).is_err());
    }

    fn layout_snapshot() -> rootcause::Result<LayoutSnapshot> {
        let active_tab = TabId::new(1)?;
        let active_pane = PaneId::new(1)?;
        let pane = PaneSnapshot {
            agent_state: PaneAgentState::NoAgent,
            cwd: "/tmp".to_owned(),
            cmd_label: None,
            id: active_pane,
            title: "shell".to_owned(),
        };
        let tab = TabSnapshot::new(active_tab, "default", active_pane, vec![pane])?;
        LayoutSnapshot::new(active_tab, vec![tab])
    }

    fn raw_tab_snapshot(id: u32, title: &str, active_pane: u32, panes: Vec<PaneSnapshot>) -> TabSnapshot {
        test_helpers::raw_tab_snapshot(tab_id(id), title, pane_id(active_pane), panes)
    }

    fn raw_pane_snapshot(id: u32, title: &str) -> PaneSnapshot {
        PaneSnapshot {
            agent_state: PaneAgentState::NoAgent,
            cwd: "/tmp".to_owned(),
            cmd_label: None,
            id: pane_id(id),
            title: title.to_owned(),
        }
    }

    fn tab_id(id: u32) -> TabId {
        TabId::new(id).expect("test tab id should be valid")
    }

    fn pane_id(id: u32) -> PaneId {
        PaneId::new(id).expect("test pane id should be valid")
    }
}
