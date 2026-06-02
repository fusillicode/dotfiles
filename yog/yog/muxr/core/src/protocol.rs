use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::num::NonZeroU16;

use rkyv::util::AlignedVec;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

use crate::SessionName;

const PROTOCOL_FRAME_MAGIC: &[u8; 9] = b"MUXR-RKYV";

/// PTY terminal dimensions with nonzero columns and rows.
#[derive(rkyv::Archive, Clone, Debug, Deserialize, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct TerminalSize {
    cols: NonZeroU16,
    rows: NonZeroU16,
}

impl TerminalSize {
    /// Build terminal dimensions, rejecting zero values before they reach the PTY layer.
    ///
    /// # Errors
    /// - Columns or rows are zero.
    pub fn new(cols: u16, rows: u16) -> rootcause::Result<Self> {
        let Some(cols) = NonZeroU16::new(cols) else {
            return Err(report!("invalid muxr terminal size").attach("cols=0"));
        };
        let Some(rows) = NonZeroU16::new(rows) else {
            return Err(report!("invalid muxr terminal size").attach("rows=0"));
        };

        Ok(Self { cols, rows })
    }

    /// Return terminal columns.
    #[must_use]
    pub const fn cols(&self) -> u16 {
        self.cols.get()
    }

    /// Return terminal rows.
    #[must_use]
    pub const fn rows(&self) -> u16 {
        self.rows.get()
    }
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
#[serde(transparent)]
pub struct TabId(String);

impl TabId {
    /// Build a tab id for layout snapshots and persisted session state.
    ///
    /// # Errors
    /// - The id is empty, reserved, too long, or contains unsupported characters.
    pub fn new(id: impl Into<String>) -> rootcause::Result<Self> {
        let id = id.into();
        self::validate_layout_id("tab", &id)?;
        Ok(Self(id))
    }
}

impl<'de> Deserialize<'de> for TabId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl AsRef<str> for TabId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl<D> rkyv::Deserialize<TabId, D> for ArchivedTabId
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<TabId, D::Error> {
        let raw = rkyv::Deserialize::<String, D>::deserialize(&self.0, deserializer)?;
        TabId::new(raw).map_err(self::rkyv_deserialize_error::<D::Error>)
    }
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
#[serde(transparent)]
pub struct PaneId(String);

impl PaneId {
    /// Build a pane id for layout snapshots and persisted session state.
    ///
    /// # Errors
    /// - The id is empty, reserved, too long, or contains unsupported characters.
    pub fn new(id: impl Into<String>) -> rootcause::Result<Self> {
        let id = id.into();
        self::validate_layout_id("pane", &id)?;
        Ok(Self(id))
    }
}

impl<'de> Deserialize<'de> for PaneId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl AsRef<str> for PaneId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl<D> rkyv::Deserialize<PaneId, D> for ArchivedPaneId
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<PaneId, D::Error> {
        let raw = rkyv::Deserialize::<String, D>::deserialize(&self.0, deserializer)?;
        PaneId::new(raw).map_err(self::rkyv_deserialize_error::<D::Error>)
    }
}

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct AttachRequest {
    pub session: SessionName,
    pub terminal_size: TerminalSize,
}

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct AttachAccepted {
    pub layout: LayoutSnapshot,
    pub pane_regions: PaneRegionsSnapshot,
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
    /// - Any tab or pane id is invalid or duplicated.
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
        self::validate_layout_id("tab", self.active_tab.as_ref())?;
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
            if !seen_tab_ids.insert(tab.id.as_ref()) {
                return Err(report!("invalid muxr layout snapshot")
                    .attach("reason=duplicate tab id")
                    .attach(format!("tab_id={}", tab.id)));
            }

            for pane in &tab.panes {
                if !seen_pane_ids.insert(pane.id.as_ref()) {
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
        LayoutSnapshot::new(active_tab, tabs).map_err(self::rkyv_deserialize_error::<D::Error>)
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
    /// - The tab or pane id is invalid or duplicated.
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
        self::validate_layout_id("tab", self.id.as_ref())?;
        self::validate_layout_id("pane", self.active_pane.as_ref())?;
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
            if !seen_pane_ids.insert(pane.id.as_ref()) {
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
        TabSnapshot::new(id, title, active_pane, panes).map_err(self::rkyv_deserialize_error::<D::Error>)
    }
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct PaneSnapshot {
    /// Current pane working directory, used by the client tab bar.
    pub cwd: String,
    /// Shell-provided command label from the pane terminal title, used by the client tab bar.
    pub command_label: Option<String>,
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
        let cwd = rkyv::Deserialize::<String, D>::deserialize(&self.cwd, deserializer)?;
        let command_label = rkyv::Deserialize::<Option<String>, D>::deserialize(&self.command_label, deserializer)?;
        let id = rkyv::Deserialize::<PaneId, D>::deserialize(&self.id, deserializer)?;
        let title = rkyv::Deserialize::<String, D>::deserialize(&self.title, deserializer)?;
        Ok(PaneSnapshot {
            cwd,
            command_label,
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
    /// - The pane id is invalid.
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
        self::validate_layout_id("pane", self.id.as_ref())?;
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
            .map_err(self::rkyv_deserialize_error::<D::Error>)
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
            if !seen_pane_ids.insert(region.id.as_ref()) {
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
        PaneRegionsSnapshot::new(regions).map_err(self::rkyv_deserialize_error::<D::Error>)
    }
}

fn validate_layout_id(kind: &'static str, id: &str) -> rootcause::Result<()> {
    if id.is_empty() {
        return Err(report!("invalid muxr layout id")
            .attach(format!("kind={kind}"))
            .attach("reason=ids must not be empty"));
    }
    if matches!(id, "." | "..") {
        return Err(report!("invalid muxr layout id")
            .attach(format!("kind={kind}"))
            .attach("reason=reserved ids are not allowed")
            .attach(format!("id={id:?}")));
    }
    if id.len() > 64 {
        return Err(report!("invalid muxr layout id")
            .attach(format!("kind={kind}"))
            .attach("reason=ids longer than 64 bytes are not allowed"));
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(report!("invalid muxr layout id")
            .attach(format!("kind={kind}"))
            .attach("reason=only ASCII alphanumeric, _, -, and . are allowed")
            .attach(format!("id={id:?}")));
    }

    Ok(())
}

fn rkyv_deserialize_error<E>(error: impl fmt::Display) -> E
where
    E: rkyv::rancor::Source,
{
    <E as rkyv::rancor::Source>::new(io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum RenderUpdate {
    Baseline(RenderBaseline),
    Diff(RenderDiff),
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct RenderBaseline {
    cursor: RenderCursor,
    rows: Vec<RenderRowSpan>,
    seq: u64,
    size: TerminalSize,
}

impl RenderBaseline {
    /// Build a full visible-frame render baseline.
    ///
    /// # Errors
    /// - The sequence is zero.
    /// - The cursor or any row span is outside the frame size.
    /// - The baseline does not contain exactly one full-width row for every visible row.
    pub fn new(
        seq: u64,
        size: TerminalSize,
        cursor: RenderCursor,
        rows: Vec<RenderRowSpan>,
    ) -> rootcause::Result<Self> {
        let baseline = Self {
            cursor,
            rows,
            seq,
            size,
        };
        baseline.validate()?;
        Ok(baseline)
    }

    #[must_use]
    pub fn into_parts(self) -> (u64, TerminalSize, RenderCursor, Vec<RenderRowSpan>) {
        (self.seq, self.size, self.cursor, self.rows)
    }

    #[must_use]
    pub const fn cursor(&self) -> &RenderCursor {
        &self.cursor
    }

    #[must_use]
    pub fn rows(&self) -> &[RenderRowSpan] {
        &self.rows
    }

    #[must_use]
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    #[must_use]
    pub const fn size(&self) -> &TerminalSize {
        &self.size
    }

    fn validate(&self) -> rootcause::Result<()> {
        if self.seq == 0 {
            return Err(report!("invalid muxr render baseline").attach("reason=seq must be nonzero"));
        }
        self.cursor.validate(self.size.rows(), self.size.cols())?;
        Self::validate_full_rows(&self.size, &self.rows)
    }

    fn validate_full_rows(size: &TerminalSize, rows: &[RenderRowSpan]) -> rootcause::Result<()> {
        if rows.len() != usize::from(size.rows()) {
            return Err(report!("invalid muxr render baseline")
                .attach("reason=row count must match frame height")
                .attach(format!("expected={}", size.rows()))
                .attach(format!("actual={}", rows.len())));
        }

        for (expected_row, row) in (0..size.rows()).zip(rows.iter()) {
            row.validate(size.rows(), size.cols())?;
            if row.row != expected_row || row.col != 0 || row.width()? != size.cols() {
                return Err(report!("invalid muxr render baseline")
                    .attach("reason=baseline rows must be full-width and ordered")
                    .attach(format!("expected_row={expected_row}"))
                    .attach(format!("actual_row={}", row.row)));
            }
        }

        Ok(())
    }
}

impl<D> rkyv::Deserialize<RenderBaseline, D> for ArchivedRenderBaseline
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<RenderBaseline, D::Error> {
        let cursor = rkyv::Deserialize::<RenderCursor, D>::deserialize(&self.cursor, deserializer)?;
        let rows = rkyv::Deserialize::<Vec<RenderRowSpan>, D>::deserialize(&self.rows, deserializer)?;
        let seq = rkyv::Deserialize::<u64, D>::deserialize(&self.seq, deserializer)?;
        let size = rkyv::Deserialize::<TerminalSize, D>::deserialize(&self.size, deserializer)?;
        RenderBaseline::new(seq, size, cursor, rows).map_err(self::rkyv_deserialize_error::<D::Error>)
    }
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct RenderDiff {
    base_seq: u64,
    cursor: RenderCursor,
    rows: Vec<RenderRowSpan>,
    seq: u64,
    size: TerminalSize,
}

impl RenderDiff {
    /// Build a dirty-row render update against a previous sequence.
    ///
    /// # Errors
    /// - The base sequence is zero.
    /// - The new sequence does not advance the base sequence.
    /// - The cursor or any dirty row span is outside the frame size.
    pub fn new(
        base_seq: u64,
        seq: u64,
        size: TerminalSize,
        cursor: RenderCursor,
        rows: Vec<RenderRowSpan>,
    ) -> rootcause::Result<Self> {
        let diff = Self {
            base_seq,
            cursor,
            rows,
            seq,
            size,
        };
        diff.validate()?;
        Ok(diff)
    }

    #[must_use]
    pub fn into_parts(self) -> (u64, u64, TerminalSize, RenderCursor, Vec<RenderRowSpan>) {
        (self.base_seq, self.seq, self.size, self.cursor, self.rows)
    }

    #[must_use]
    pub const fn base_seq(&self) -> u64 {
        self.base_seq
    }

    #[must_use]
    pub const fn cursor(&self) -> &RenderCursor {
        &self.cursor
    }

    #[must_use]
    pub fn rows(&self) -> &[RenderRowSpan] {
        &self.rows
    }

    #[must_use]
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    #[must_use]
    pub const fn size(&self) -> &TerminalSize {
        &self.size
    }

    fn validate(&self) -> rootcause::Result<()> {
        if self.base_seq == 0 {
            return Err(report!("invalid muxr render diff").attach("reason=base_seq must be nonzero"));
        }
        if self.seq <= self.base_seq {
            return Err(report!("invalid muxr render diff")
                .attach("reason=seq must advance base_seq")
                .attach(format!("base_seq={}", self.base_seq))
                .attach(format!("seq={}", self.seq)));
        }
        self.cursor.validate(self.size.rows(), self.size.cols())?;
        for row in &self.rows {
            row.validate(self.size.rows(), self.size.cols())?;
        }

        Ok(())
    }
}

impl<D> rkyv::Deserialize<RenderDiff, D> for ArchivedRenderDiff
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<RenderDiff, D::Error> {
        let base_seq = rkyv::Deserialize::<u64, D>::deserialize(&self.base_seq, deserializer)?;
        let cursor = rkyv::Deserialize::<RenderCursor, D>::deserialize(&self.cursor, deserializer)?;
        let rows = rkyv::Deserialize::<Vec<RenderRowSpan>, D>::deserialize(&self.rows, deserializer)?;
        let seq = rkyv::Deserialize::<u64, D>::deserialize(&self.seq, deserializer)?;
        let size = rkyv::Deserialize::<TerminalSize, D>::deserialize(&self.size, deserializer)?;
        RenderDiff::new(base_seq, seq, size, cursor, rows).map_err(self::rkyv_deserialize_error::<D::Error>)
    }
}

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct RenderCursor {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
}

impl RenderCursor {
    fn validate(&self, rows: u16, cols: u16) -> rootcause::Result<()> {
        if !self.visible {
            return Ok(());
        }
        if self.row >= rows || self.col >= cols {
            return Err(report!("invalid muxr render cursor")
                .attach(format!("row={}", self.row))
                .attach(format!("col={}", self.col))
                .attach(format!("rows={rows}"))
                .attach(format!("cols={cols}")));
        }

        Ok(())
    }
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct RenderRowSpan {
    cells: Vec<RenderCell>,
    col: u16,
    row: u16,
}

impl RenderRowSpan {
    /// Build a row span with nonempty cells and valid wide-cell pairing.
    ///
    /// # Errors
    /// - The row span has no cells.
    /// - A wide cell is not followed by exactly one continuation cell.
    pub fn new(row: u16, col: u16, cells: Vec<RenderCell>) -> rootcause::Result<Self> {
        let span = Self { cells, col, row };
        span.validate_cells()?;
        Ok(span)
    }

    #[must_use]
    pub fn cells(&self) -> &[RenderCell] {
        &self.cells
    }

    #[must_use]
    pub const fn col(&self) -> u16 {
        self.col
    }

    #[must_use]
    pub const fn row(&self) -> u16 {
        self.row
    }

    /// Return the number of terminal grid cells covered by this row span.
    ///
    /// # Errors
    /// - The cell count does not fit in `u16`.
    pub fn width(&self) -> rootcause::Result<u16> {
        let width = self.cells.len();
        Ok(u16::try_from(width).context("muxr render row span width overflowed")?)
    }

    fn validate(&self, rows: u16, cols: u16) -> rootcause::Result<()> {
        self.validate_cells()?;
        let width = self.width()?;
        let Some(end_col) = self.col.checked_add(width) else {
            return Err(report!("invalid muxr render row span").attach("reason=column range overflowed"));
        };
        if self.row >= rows || self.col >= cols || end_col > cols {
            return Err(report!("invalid muxr render row span")
                .attach(format!("row={}", self.row))
                .attach(format!("col={}", self.col))
                .attach(format!("width={width}"))
                .attach(format!("rows={rows}"))
                .attach(format!("cols={cols}")));
        }
        Ok(())
    }

    fn validate_cells(&self) -> rootcause::Result<()> {
        if self.cells.is_empty() {
            return Err(report!("invalid muxr render row span").attach("reason=cells must not be empty"));
        }
        self.validate_wide_cells()
    }

    fn validate_wide_cells(&self) -> rootcause::Result<()> {
        let mut wide_cell_index = None;

        for (index, cell) in self.cells.iter().enumerate() {
            if wide_cell_index.is_some() {
                match cell.width {
                    RenderCellWidth::WideContinuation => {
                        wide_cell_index = None;
                        continue;
                    }
                    RenderCellWidth::Narrow | RenderCellWidth::Wide => {
                        return Err(self::invalid_wide_cell_sequence(
                            "wide cell must be followed by a wide continuation",
                            index,
                        ));
                    }
                }
            }

            match cell.width {
                RenderCellWidth::Narrow => {}
                RenderCellWidth::Wide => {
                    wide_cell_index = Some(index);
                }
                RenderCellWidth::WideContinuation => {
                    return Err(self::invalid_wide_cell_sequence(
                        "wide continuation must follow a wide cell",
                        index,
                    ));
                }
            }
        }

        if let Some(index) = wide_cell_index {
            return Err(self::invalid_wide_cell_sequence(
                "wide cell must be followed by a wide continuation",
                index,
            ));
        }

        Ok(())
    }
}

impl<D> rkyv::Deserialize<RenderRowSpan, D> for ArchivedRenderRowSpan
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<RenderRowSpan, D::Error> {
        let cells = rkyv::Deserialize::<Vec<RenderCell>, D>::deserialize(&self.cells, deserializer)?;
        let col = rkyv::Deserialize::<u16, D>::deserialize(&self.col, deserializer)?;
        let row = rkyv::Deserialize::<u16, D>::deserialize(&self.row, deserializer)?;
        RenderRowSpan::new(row, col, cells).map_err(self::rkyv_deserialize_error::<D::Error>)
    }
}

fn invalid_wide_cell_sequence(reason: &'static str, index: usize) -> rootcause::Report {
    report!("invalid muxr render row span")
        .attach("reason=invalid wide-cell sequence")
        .attach(reason)
        .attach(format!("cell_index={index}"))
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct RenderCell {
    style: RenderStyle,
    text: String,
    width: RenderCellWidth,
}

impl RenderCell {
    #[must_use]
    pub fn narrow(text: impl Into<String>, style: RenderStyle) -> Self {
        Self {
            style,
            text: text.into(),
            width: RenderCellWidth::Narrow,
        }
    }

    #[must_use]
    pub fn wide(text: impl Into<String>, style: RenderStyle) -> Self {
        Self {
            style,
            text: text.into(),
            width: RenderCellWidth::Wide,
        }
    }

    #[must_use]
    pub const fn wide_continuation(style: RenderStyle) -> Self {
        Self {
            style,
            text: String::new(),
            width: RenderCellWidth::WideContinuation,
        }
    }

    #[must_use]
    pub const fn style(&self) -> RenderStyle {
        self.style
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn width(&self) -> RenderCellWidth {
        self.width
    }
}

impl<D> rkyv::Deserialize<RenderCell, D> for ArchivedRenderCell
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<RenderCell, D::Error> {
        let style = rkyv::Deserialize::<RenderStyle, D>::deserialize(&self.style, deserializer)?;
        let text = rkyv::Deserialize::<String, D>::deserialize(&self.text, deserializer)?;
        let width = rkyv::Deserialize::<RenderCellWidth, D>::deserialize(&self.width, deserializer)?;
        match width {
            RenderCellWidth::Narrow => Ok(RenderCell::narrow(text, style)),
            RenderCellWidth::Wide => Ok(RenderCell::wide(text, style)),
            RenderCellWidth::WideContinuation => {
                if !text.is_empty() {
                    return Err(self::rkyv_deserialize_error::<D::Error>(
                        "wide continuation cells must not carry text",
                    ));
                }
                Ok(RenderCell::wide_continuation(style))
            }
        }
    }
}

#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum RenderCellWidth {
    Narrow,
    Wide,
    WideContinuation,
}

#[derive(rkyv::Archive, Clone, Copy, Debug, Default, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct RenderStyle {
    pub attrs: RenderTextStyle,
    pub bg: RenderColor,
    pub fg: RenderColor,
}

#[derive(rkyv::Archive, Clone, Copy, Debug, Default, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
#[serde(transparent)]
pub struct RenderTextStyle(u8);

impl RenderTextStyle {
    const BOLD: u8 = 0b0000_0001;
    const DIM: u8 = 0b0000_0010;
    const INVERSE: u8 = 0b0001_0000;
    const ITALIC: u8 = 0b0000_0100;
    const UNDERLINE: u8 = 0b0000_1000;

    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn set_bold(self, enabled: bool) -> Self {
        self.set_flag(Self::BOLD, enabled)
    }

    #[must_use]
    pub const fn set_dim(self, enabled: bool) -> Self {
        self.set_flag(Self::DIM, enabled)
    }

    #[must_use]
    pub const fn set_italic(self, enabled: bool) -> Self {
        self.set_flag(Self::ITALIC, enabled)
    }

    #[must_use]
    pub const fn set_underline(self, enabled: bool) -> Self {
        self.set_flag(Self::UNDERLINE, enabled)
    }

    #[must_use]
    pub const fn set_inverse(self, enabled: bool) -> Self {
        self.set_flag(Self::INVERSE, enabled)
    }

    #[must_use]
    pub const fn bold(self) -> bool {
        self.has_flag(Self::BOLD)
    }

    #[must_use]
    pub const fn dim(self) -> bool {
        self.has_flag(Self::DIM)
    }

    #[must_use]
    pub const fn italic(self) -> bool {
        self.has_flag(Self::ITALIC)
    }

    #[must_use]
    pub const fn underline(self) -> bool {
        self.has_flag(Self::UNDERLINE)
    }

    #[must_use]
    pub const fn inverse(self) -> bool {
        self.has_flag(Self::INVERSE)
    }

    const fn set_flag(self, flag: u8, enabled: bool) -> Self {
        if enabled {
            Self(self.0 | flag)
        } else {
            Self(self.0 & !flag)
        }
    }

    const fn has_flag(self, flag: u8) -> bool {
        self.0 & flag != 0
    }
}

#[derive(rkyv::Archive, Clone, Copy, Debug, Default, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum RenderColor {
    #[default]
    Default,
    /// Xterm 256-color palette index (`38;5;<n>` / `48;5;<n>`), used when terminal-theme-relative colors are enough.
    Indexed(u8),
    /// Explicit 24-bit RGB color (`38;2;r;g;b` / `48;2;r;g;b`) for exact colors.
    Rgb { r: u8, g: u8, b: u8 },
}

/// Normalized key code carried with the original terminal bytes.
#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum ClientKeyCode {
    Backspace,
    Char(char),
    Down,
    Enter,
    Esc,
    Left,
    Right,
    Tab,
    Unknown,
    Up,
}

/// Keyboard modifiers observed by the muxr client.
#[derive(rkyv::Archive, Clone, Copy, Debug, Default, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct ClientKeyModifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
}

impl ClientKeyModifiers {
    pub const ALT: Self = Self {
        alt: true,
        ctrl: false,
        shift: false,
    };
    pub const CTRL_ALT: Self = Self {
        alt: true,
        ctrl: true,
        shift: false,
    };
    pub const NONE: Self = Self {
        alt: false,
        ctrl: false,
        shift: false,
    };
    pub const SHIFT_ALT: Self = Self {
        alt: true,
        ctrl: false,
        shift: true,
    };
}

/// One ordered keyboard event from the muxr client.
#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct ClientKey {
    pub code: ClientKeyCode,
    pub modifiers: ClientKeyModifiers,
    pub raw_bytes: Vec<u8>,
}

#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum PaneScrollDirection {
    Down,
    Up,
}

#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct ClientMousePosition {
    /// Zero-based row in the client viewport.
    pub row: u16,
    /// Zero-based column in the client viewport.
    pub col: u16,
}

/// Press or release phase for an SGR mouse event captured by the muxr client.
#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum ClientMouseEventPhase {
    /// Button press, wheel, or button-motion event.
    Press,
    /// Button release event.
    Release,
}

/// Mouse event captured from the outer terminal before server-side pane translation.
#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct ClientMouseEvent {
    /// SGR button code, including modifier, wheel, and motion bits.
    pub button: u16,
    /// Press/motion/wheel or release phase.
    pub phase: ClientMouseEventPhase,
    /// Position in client viewport coordinates.
    pub position: ClientMousePosition,
}

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
#[serde(tag = "code", content = "msg", rename_all = "snake_case")]
pub enum ServerError {
    ClientAlreadyAttached,
    SessionMismatch { expected: SessionName, actual: SessionName },
    UnexpectedRequest { request: Box<ClientRequest> },
}

impl ServerError {
    #[must_use]
    pub fn unexpected_request(request: ClientRequest) -> Self {
        Self::UnexpectedRequest {
            request: Box::new(request),
        }
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ClientAlreadyAttached => "client_already_attached",
            Self::SessionMismatch { .. } => "session_mismatch",
            Self::UnexpectedRequest { .. } => "unexpected_request",
        }
    }

    #[must_use]
    pub fn msg(&self) -> String {
        match self {
            Self::ClientAlreadyAttached => "a muxr client is already attached to this session".to_owned(),
            Self::SessionMismatch { expected, actual } => format!("expected session {expected}, got {actual}"),
            Self::UnexpectedRequest { request } => format!("unexpected client request during attach: {request:?}"),
        }
    }
}

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum ClientRequest {
    Attach(AttachRequest),
    DeleteSession,
    Ping,
    Pong,
    Detach,
    RenderResync,
    Resize(TerminalSize),
    Input(Vec<u8>),
    Paste(Vec<u8>),
    Key(ClientKey),
    Mouse(ClientMouseEvent),
    ScrollPaneAt {
        position: ClientMousePosition,
        direction: PaneScrollDirection,
    },
    ScrollPaneLineAt {
        position: ClientMousePosition,
        direction: PaneScrollDirection,
    },
    FocusPaneAt(ClientMousePosition),
    FocusTab(TabId),
}

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum ServerEvent {
    Attached(AttachAccepted),
    Deleted,
    Ping,
    Pong,
    Layout(LayoutSnapshot),
    PaneRegions(PaneRegionsSnapshot),
    Render(RenderUpdate),
    Error(ServerError),
    Detached,
}

/// Encode a client request as a rkyv protocol payload.
///
/// # Errors
/// - The request cannot be serialized.
pub fn encode_client_request(request: &ClientRequest) -> rootcause::Result<Vec<u8>> {
    let payload = rkyv::to_bytes::<rkyv::rancor::Error>(request)
        .map_err(|error| report!("failed to serialize muxr protocol frame").attach(format!("{error:?}")))?;
    Ok(self::encode_protocol_frame(payload.as_slice()))
}

/// Decode a client request from one rkyv protocol payload.
///
/// # Errors
/// - The frame is empty or not a valid client request payload.
/// - The decoded request cannot be deserialized into valid domain values.
pub fn decode_client_request(line: &[u8]) -> rootcause::Result<ClientRequest> {
    let payload = self::decode_protocol_payload(line)?;
    let archived = rkyv::access::<rkyv::Archived<ClientRequest>, rkyv::rancor::Error>(&payload)
        .map_err(|error| report!("failed to validate muxr protocol frame").attach(format!("{error:?}")))?;
    rkyv::deserialize::<ClientRequest, rkyv::rancor::Error>(archived)
        .map_err(|error| report!("failed to deserialize muxr protocol frame").attach(format!("{error:?}")))
}

/// Encode a server event as a rkyv protocol payload.
///
/// # Errors
/// - The event cannot be serialized.
pub fn encode_server_event(event: &ServerEvent) -> rootcause::Result<Vec<u8>> {
    let payload = rkyv::to_bytes::<rkyv::rancor::Error>(event)
        .map_err(|error| report!("failed to serialize muxr protocol frame").attach(format!("{error:?}")))?;
    Ok(self::encode_protocol_frame(payload.as_slice()))
}

/// Decode a server event from one rkyv protocol payload.
///
/// # Errors
/// - The frame is empty or not a valid server event payload.
/// - The decoded event cannot be deserialized into valid domain values.
pub fn decode_server_event(line: &[u8]) -> rootcause::Result<ServerEvent> {
    let payload = self::decode_protocol_payload(line)?;
    let archived = rkyv::access::<rkyv::Archived<ServerEvent>, rkyv::rancor::Error>(&payload)
        .map_err(|error| report!("failed to validate muxr protocol frame").attach(format!("{error:?}")))?;
    rkyv::deserialize::<ServerEvent, rkyv::rancor::Error>(archived)
        .map_err(|error| report!("failed to deserialize muxr protocol frame").attach(format!("{error:?}")))
}

fn encode_protocol_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = PROTOCOL_FRAME_MAGIC.to_vec();
    frame.extend_from_slice(payload);
    frame
}

fn decode_protocol_payload(frame: &[u8]) -> rootcause::Result<AlignedVec> {
    if frame.is_empty() {
        return Err(report!("empty muxr protocol frame"));
    }
    let Some(payload) = frame.strip_prefix(PROTOCOL_FRAME_MAGIC) else {
        return Err(report!("invalid muxr protocol frame")
            .attach("reason=missing rkyv frame magic")
            .attach(format!("magic={PROTOCOL_FRAME_MAGIC:?}")));
    };
    if payload.is_empty() {
        return Err(report!("empty muxr protocol payload"));
    }
    // Socket buffers have arbitrary byte alignment; rkyv checked access requires aligned archived bytes.
    let mut aligned = AlignedVec::with_capacity(payload.len());
    aligned.extend_from_slice(payload);
    Ok(aligned)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::attach(ClientRequest::Attach(client_attach_request()?))]
    #[case::delete_session(ClientRequest::DeleteSession)]
    #[case::ping(ClientRequest::Ping)]
    #[case::pong(ClientRequest::Pong)]
    #[case::detach(ClientRequest::Detach)]
    #[case::render_resync(ClientRequest::RenderResync)]
    #[case::resize(ClientRequest::Resize(terminal_size(120, 40)?))]
    #[case::input(ClientRequest::Input(vec![b'a', b'b', b'\n']))]
    #[case::paste(ClientRequest::Paste(vec![b'a', b'\n', b'b', b'\n']))]
    #[case::key(ClientRequest::Key(client_key()))]
    #[case::mouse(ClientRequest::Mouse(ClientMouseEvent {
        button: 0,
        phase: ClientMouseEventPhase::Press,
        position: ClientMousePosition { row: 2, col: 3 },
    }))]
    #[case::scroll(ClientRequest::ScrollPaneAt {
        position: ClientMousePosition { row: 2, col: 3 },
        direction: PaneScrollDirection::Up,
    })]
    #[case::scroll_line(ClientRequest::ScrollPaneLineAt {
        position: ClientMousePosition { row: 2, col: 3 },
        direction: PaneScrollDirection::Down,
    })]
    #[case::focus_pane_at(ClientRequest::FocusPaneAt(ClientMousePosition { row: 2, col: 3 }))]
    #[case::focus_tab(ClientRequest::FocusTab(TabId::new("tab-2")?))]
    fn test_client_request_codec_when_frame_round_trips_returns_original(
        #[case] request: ClientRequest,
    ) -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(decode_client_request(&encode_client_request(&request)?)?, request);
        Ok(())
    }

    #[rstest]
    #[case::attached(ServerEvent::Attached(attach_accepted()?))]
    #[case::deleted(ServerEvent::Deleted)]
    #[case::ping(ServerEvent::Ping)]
    #[case::pong(ServerEvent::Pong)]
    #[case::layout(ServerEvent::Layout(layout_snapshot()?))]
    #[case::pane_regions(ServerEvent::PaneRegions(pane_regions_snapshot()?))]
    #[case::render_baseline(ServerEvent::Render(RenderUpdate::Baseline(render_baseline()?)))]
    #[case::render_diff(ServerEvent::Render(RenderUpdate::Diff(render_diff()?)))]
    #[case::error(ServerEvent::Error(ServerError::unexpected_request(ClientRequest::Detach)))]
    #[case::detached(ServerEvent::Detached)]
    fn test_server_event_codec_when_frame_round_trips_returns_original(
        #[case] event: ServerEvent,
    ) -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(decode_server_event(&encode_server_event(&event)?)?, event);
        Ok(())
    }

    #[test]
    fn test_server_event_codec_when_render_update_is_invalid_returns_error() -> rootcause::Result<()> {
        let event = invalid_render_event()?;
        let encoded = encode_server_event(&event)?;

        assert2::assert!(decode_server_event(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn test_server_event_codec_when_attached_layout_is_invalid_returns_error() -> rootcause::Result<()> {
        let event = ServerEvent::Attached(AttachAccepted {
            layout: LayoutSnapshot {
                active_tab: TabId::new("missing")?,
                tabs: vec![tab_snapshot(
                    "tab-1",
                    "default",
                    "pane-1",
                    vec![pane_snapshot("pane-1", "shell")?],
                )?],
            },
            pane_regions: pane_regions_snapshot()?,
        });
        let encoded = encode_server_event(&event)?;

        assert2::assert!(decode_server_event(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn test_server_event_codec_when_layout_event_is_invalid_returns_error() -> rootcause::Result<()> {
        let event = ServerEvent::Layout(LayoutSnapshot {
            active_tab: TabId::new("missing")?,
            tabs: vec![tab_snapshot(
                "tab-1",
                "default",
                "pane-1",
                vec![pane_snapshot("pane-1", "shell")?],
            )?],
        });
        let encoded = encode_server_event(&event)?;

        assert2::assert!(decode_server_event(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn test_client_request_codec_when_frame_magic_is_missing_returns_error() {
        let encoded = b"not-muxr-rkyv";

        assert2::assert!(decode_client_request(encoded).is_err());
    }

    #[rstest]
    #[case::zero_cols(r#"{"cols":0,"rows":24}"#)]
    #[case::zero_rows(r#"{"cols":80,"rows":0}"#)]
    fn test_terminal_size_deserialize_when_dimension_is_zero_returns_error(#[case] raw: &str) {
        assert2::assert!(serde_json::from_str::<TerminalSize>(raw).is_err());
    }

    #[rstest]
    #[case::zero_cols(0, 24)]
    #[case::zero_rows(80, 0)]
    fn test_terminal_size_new_when_dimension_is_zero_returns_error(#[case] cols: u16, #[case] rows: u16) {
        assert2::assert!(TerminalSize::new(cols, rows).is_err());
    }

    #[test]
    fn test_terminal_size_new_when_dimensions_are_nonzero_returns_size() -> rootcause::Result<()> {
        let size = TerminalSize::new(120, 40)?;

        pretty_assertions::assert_eq!(size.cols(), 120);
        pretty_assertions::assert_eq!(size.rows(), 40);
        Ok(())
    }

    #[test]
    fn test_layout_snapshot_single_pane_when_built_returns_stable_layout() -> rootcause::Result<()> {
        let layout = layout_snapshot()?;

        pretty_assertions::assert_eq!(layout.active_tab.as_ref(), "tab-1");
        pretty_assertions::assert_eq!(layout.tabs.len(), 1);
        let Some(tab) = layout.tabs.first() else {
            return Err(report!("expected one tab"));
        };
        pretty_assertions::assert_eq!(tab.active_pane.as_ref(), "pane-1");
        pretty_assertions::assert_eq!(tab.panes.len(), 1);
        Ok(())
    }

    #[rstest]
    #[case::empty("")]
    #[case::dot(".")]
    #[case::dot_dot("..")]
    #[case::forward_slash("a/b")]
    #[case::backslash("a\\b")]
    #[case::space("a b")]
    #[case::tab("a\tb")]
    #[case::shell_metacharacters("$(x)")]
    #[case::punctuation("name!")]
    fn test_layout_id_new_when_id_is_invalid_returns_error(#[case] raw: &str) {
        assert2::assert!(TabId::new(raw).is_err());
        assert2::assert!(PaneId::new(raw).is_err());
    }

    #[test]
    fn test_layout_id_new_when_id_is_too_long_returns_error() {
        let raw = "a".repeat(65);

        assert2::assert!(TabId::new(raw.clone()).is_err());
        assert2::assert!(PaneId::new(raw).is_err());
    }

    #[rstest]
    #[case::slash(r#""a/b""#)]
    #[case::reserved(r#"".""#)]
    fn test_layout_id_deserialize_when_id_is_invalid_returns_error(#[case] raw: &str) {
        assert2::assert!(serde_json::from_str::<TabId>(raw).is_err());
        assert2::assert!(serde_json::from_str::<PaneId>(raw).is_err());
    }

    #[rstest]
    #[case::empty_tabs(LayoutSnapshot {
        active_tab: tab_id("tab-1"),
        tabs: Vec::new(),
    })]
    #[case::missing_active_tab(LayoutSnapshot {
        active_tab: tab_id("missing"),
        tabs: vec![raw_tab_snapshot("tab-1", "default", "pane-1", vec![raw_pane_snapshot("pane-1", "shell")])],
    })]
    #[case::empty_panes(LayoutSnapshot {
        active_tab: tab_id("tab-1"),
        tabs: vec![TabSnapshot {
            active_pane: pane_id("pane-1"),
            id: tab_id("tab-1"),
            panes: Vec::new(),
            title: "default".to_owned(),
        }],
    })]
    #[case::missing_active_pane(LayoutSnapshot {
        active_tab: tab_id("tab-1"),
        tabs: vec![TabSnapshot {
            active_pane: pane_id("missing"),
            id: tab_id("tab-1"),
            panes: vec![raw_pane_snapshot("pane-1", "shell")],
            title: "default".to_owned(),
        }],
    })]
    #[case::duplicate_tab(LayoutSnapshot {
        active_tab: tab_id("tab-1"),
        tabs: vec![
            raw_tab_snapshot("tab-1", "default", "pane-1", vec![raw_pane_snapshot("pane-1", "shell")]),
            raw_tab_snapshot("tab-1", "other", "pane-2", vec![raw_pane_snapshot("pane-2", "shell")]),
        ],
    })]
    #[case::duplicate_pane(LayoutSnapshot {
        active_tab: tab_id("tab-1"),
        tabs: vec![TabSnapshot {
            active_pane: pane_id("pane-1"),
            id: tab_id("tab-1"),
            panes: vec![raw_pane_snapshot("pane-1", "shell"), raw_pane_snapshot("pane-1", "other")],
            title: "default".to_owned(),
        }],
    })]
    #[case::duplicate_pane_across_tabs(LayoutSnapshot {
        active_tab: tab_id("tab-1"),
        tabs: vec![
            raw_tab_snapshot("tab-1", "default", "pane-1", vec![raw_pane_snapshot("pane-1", "shell")]),
            raw_tab_snapshot("tab-2", "other", "pane-2", vec![
                raw_pane_snapshot("pane-1", "other"),
                raw_pane_snapshot("pane-2", "shell"),
            ]),
        ],
    })]
    fn test_layout_snapshot_validate_when_layout_is_invalid_returns_error(#[case] layout: LayoutSnapshot) {
        assert2::assert!(layout.validate().is_err());
    }

    #[rstest]
    #[case::zero_seq(0, 80, 24, render_rows(80, 24))]
    #[case::short_rows(1, 80, 24, render_rows(80, 23))]
    #[case::out_of_bounds_row(1, 80, 24, vec![raw_render_row_span(24, 0, render_cells(80))])]
    fn test_render_baseline_new_when_frame_is_invalid_returns_error(
        #[case] seq: u64,
        #[case] cols: u16,
        #[case] rows: u16,
        #[case] render_rows: Vec<RenderRowSpan>,
    ) -> rootcause::Result<()> {
        let size = TerminalSize::new(cols, rows)?;
        assert2::assert!(
            RenderBaseline::new(
                seq,
                size,
                RenderCursor {
                    row: 0,
                    col: 0,
                    visible: true
                },
                render_rows
            )
            .is_err()
        );
        Ok(())
    }

    #[rstest]
    #[case::zero_base(0, 2)]
    #[case::same_seq(1, 1)]
    #[case::older_seq(2, 1)]
    fn test_render_diff_new_when_sequence_is_invalid_returns_error(
        #[case] base_seq: u64,
        #[case] seq: u64,
    ) -> rootcause::Result<()> {
        assert2::assert!(
            RenderDiff::new(
                base_seq,
                seq,
                TerminalSize::new(80, 24)?,
                RenderCursor {
                    row: 0,
                    col: 0,
                    visible: true
                },
                vec![RenderRowSpan::new(0, 0, render_cells(1))?],
            )
            .is_err()
        );
        Ok(())
    }

    #[rstest]
    #[case::empty_cells(raw_render_row_span(0, 0, Vec::new()))]
    #[case::col_out_of_bounds(raw_render_row_span(0, 80, render_cells(1)))]
    #[case::span_too_wide(raw_render_row_span(0, 79, render_cells(2)))]
    fn test_render_diff_new_when_row_span_is_invalid_returns_error(
        #[case] row: RenderRowSpan,
    ) -> rootcause::Result<()> {
        assert2::assert!(
            RenderDiff::new(
                1,
                2,
                TerminalSize::new(80, 24)?,
                RenderCursor {
                    row: 0,
                    col: 0,
                    visible: true
                },
                vec![row],
            )
            .is_err()
        );
        Ok(())
    }

    #[rstest]
    #[case::wide_without_continuation(raw_render_row_span(
        0,
        0,
        vec![RenderCell::wide("字", RenderStyle::default())]
    ))]
    #[case::continuation_without_wide(raw_render_row_span(
        0,
        0,
        vec![RenderCell::wide_continuation(RenderStyle::default())]
    ))]
    #[case::wide_followed_by_narrow(raw_render_row_span(
        0,
        0,
        vec![RenderCell::wide("字", RenderStyle::default()), render_cell("x")]
    ))]
    #[case::double_continuation(raw_render_row_span(
        0,
        0,
        vec![
            RenderCell::wide("字", RenderStyle::default()),
            RenderCell::wide_continuation(RenderStyle::default()),
            RenderCell::wide_continuation(RenderStyle::default())
        ]
    ))]
    fn test_render_diff_new_when_wide_cell_sequence_is_invalid_returns_error(
        #[case] row: RenderRowSpan,
    ) -> rootcause::Result<()> {
        assert2::assert!(
            RenderDiff::new(
                1,
                2,
                TerminalSize::new(80, 24)?,
                RenderCursor {
                    row: 0,
                    col: 0,
                    visible: true
                },
                vec![row],
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_render_baseline_new_when_wide_cell_sequence_is_invalid_returns_error() -> rootcause::Result<()> {
        let rows = vec![raw_render_row_span(
            0,
            0,
            vec![RenderCell::wide("字", RenderStyle::default()), render_cell("x")],
        )];

        assert2::assert!(
            RenderBaseline::new(
                1,
                TerminalSize::new(2, 1)?,
                RenderCursor {
                    row: 0,
                    col: 0,
                    visible: true
                },
                rows
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_render_diff_new_when_wide_cell_has_continuation_returns_diff() -> rootcause::Result<()> {
        let row = RenderRowSpan::new(
            0,
            0,
            vec![
                RenderCell::wide("字", RenderStyle::default()),
                RenderCell::wide_continuation(RenderStyle::default()),
            ],
        )?;

        let diff = RenderDiff::new(
            1,
            2,
            TerminalSize::new(80, 24)?,
            RenderCursor {
                row: 0,
                col: 0,
                visible: true,
            },
            vec![row],
        )?;

        pretty_assertions::assert_eq!(diff.rows.len(), 1);
        Ok(())
    }

    #[test]
    fn test_render_row_span_width_when_wide_cell_has_continuation_counts_grid_cells() -> rootcause::Result<()> {
        let row = RenderRowSpan::new(
            0,
            0,
            vec![
                RenderCell::wide("字", RenderStyle::default()),
                RenderCell::wide_continuation(RenderStyle::default()),
            ],
        )?;

        pretty_assertions::assert_eq!(row.width()?, 2);
        Ok(())
    }

    fn client_attach_request() -> rootcause::Result<AttachRequest> {
        Ok(AttachRequest {
            session: "work".parse()?,
            terminal_size: terminal_size(80, 24)?,
        })
    }

    fn attach_accepted() -> rootcause::Result<AttachAccepted> {
        Ok(AttachAccepted {
            layout: layout_snapshot()?,
            pane_regions: pane_regions_snapshot()?,
        })
    }

    fn client_key() -> ClientKey {
        ClientKey {
            code: ClientKeyCode::Char('E'),
            modifiers: ClientKeyModifiers::SHIFT_ALT,
            raw_bytes: vec![b'\x1b', b'E'],
        }
    }

    fn layout_snapshot() -> rootcause::Result<LayoutSnapshot> {
        let active_tab = TabId::new("tab-1")?;
        let active_pane = PaneId::new("pane-1")?;
        let pane = PaneSnapshot {
            cwd: "/tmp".to_owned(),
            command_label: None,
            id: active_pane.clone(),
            title: "shell".to_owned(),
        };
        let tab = TabSnapshot::new(active_tab.clone(), "default", active_pane, vec![pane])?;
        LayoutSnapshot::new(active_tab, vec![tab])
    }

    fn pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![PaneRegionSnapshot::new(
            PaneId::new("pane-1")?,
            0,
            0,
            80,
            24,
            PaneMouseMode::None,
            0,
        )?])
    }

    fn tab_snapshot(
        id: &str,
        title: &str,
        active_pane: &str,
        panes: Vec<PaneSnapshot>,
    ) -> rootcause::Result<TabSnapshot> {
        TabSnapshot::new(TabId::new(id)?, title, PaneId::new(active_pane)?, panes)
    }

    fn pane_snapshot(id: &str, title: &str) -> rootcause::Result<PaneSnapshot> {
        Ok(PaneSnapshot {
            cwd: "/tmp".to_owned(),
            command_label: None,
            id: PaneId::new(id)?,
            title: title.to_owned(),
        })
    }

    fn raw_tab_snapshot(id: &str, title: &str, active_pane: &str, panes: Vec<PaneSnapshot>) -> TabSnapshot {
        TabSnapshot {
            active_pane: pane_id(active_pane),
            id: tab_id(id),
            panes,
            title: title.to_owned(),
        }
    }

    fn raw_pane_snapshot(id: &str, title: &str) -> PaneSnapshot {
        PaneSnapshot {
            cwd: "/tmp".to_owned(),
            command_label: None,
            id: pane_id(id),
            title: title.to_owned(),
        }
    }

    fn tab_id(id: &str) -> TabId {
        TabId(id.to_owned())
    }

    fn pane_id(id: &str) -> PaneId {
        PaneId(id.to_owned())
    }

    fn terminal_size(cols: u16, rows: u16) -> rootcause::Result<TerminalSize> {
        TerminalSize::new(cols, rows)
    }

    fn render_baseline() -> rootcause::Result<RenderBaseline> {
        RenderBaseline::new(
            1,
            terminal_size(4, 2)?,
            RenderCursor {
                row: 1,
                col: 2,
                visible: true,
            },
            vec![
                RenderRowSpan::new(
                    0,
                    0,
                    vec![render_cell("a"), render_cell("b"), render_cell("c"), render_cell("d")],
                )?,
                RenderRowSpan::new(
                    1,
                    0,
                    vec![render_cell("e"), render_cell("f"), render_cell("g"), render_cell("h")],
                )?,
            ],
        )
    }

    fn render_diff() -> rootcause::Result<RenderDiff> {
        RenderDiff::new(
            1,
            2,
            terminal_size(4, 2)?,
            RenderCursor {
                row: 1,
                col: 3,
                visible: true,
            },
            vec![RenderRowSpan::new(1, 1, vec![render_cell("x"), render_cell("y")])?],
        )
    }

    fn invalid_render_event() -> rootcause::Result<ServerEvent> {
        Ok(ServerEvent::Render(RenderUpdate::Diff(RenderDiff {
            base_seq: 1,
            cursor: RenderCursor {
                row: 0,
                col: 0,
                visible: true,
            },
            rows: vec![raw_render_row_span(
                0,
                0,
                vec![RenderCell::wide_continuation(RenderStyle::default())],
            )],
            seq: 2,
            size: terminal_size(4, 2)?,
        })))
    }

    fn render_rows(cols: u16, rows: u16) -> Vec<RenderRowSpan> {
        (0..rows)
            .map(|row| raw_render_row_span(row, 0, render_cells(cols)))
            .collect()
    }

    fn raw_render_row_span(row: u16, col: u16, cells: Vec<RenderCell>) -> RenderRowSpan {
        RenderRowSpan { cells, col, row }
    }

    fn render_cells(cols: u16) -> Vec<RenderCell> {
        (0..cols).map(|_| render_cell(" ")).collect()
    }

    fn render_cell(text: &str) -> RenderCell {
        RenderCell::narrow(text, RenderStyle::default())
    }
}
