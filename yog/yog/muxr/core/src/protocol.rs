use std::fmt;
use std::num::NonZeroU16;
use std::num::NonZeroU32;

use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

use crate::SessionName;

pub const PROTOCOL_VERSION: u16 = 9;

/// PTY terminal dimensions with nonzero columns and rows.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

/// Process id for a running muxr server.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ServerPid(NonZeroU32);

impl ServerPid {
    /// Build a server pid, rejecting zero because pid `0` is not an addressable muxr server process.
    ///
    /// # Errors
    /// - The pid is zero.
    pub fn new(pid: u32) -> rootcause::Result<Self> {
        let Some(pid) = NonZeroU32::new(pid) else {
            return Err(report!("invalid muxr server pid").attach("pid=0"));
        };

        Ok(Self(pid))
    }

    /// Return the raw process id.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientHello {
    pub protocol_version: u16,
    pub session: SessionName,
    pub terminal_size: TerminalSize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServerHello {
    pub protocol_version: u16,
    pub session: SessionName,
    pub server_pid: ServerPid,
    pub layout: LayoutSnapshot,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LayoutSnapshot {
    pub active_tab: TabId,
    pub tabs: Vec<TabSnapshot>,
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

    /// Build the initial one-tab, one-pane shell layout.
    ///
    /// # Errors
    /// - The provided tab or pane id is invalid.
    pub fn single_pane(
        tab_id: impl Into<String>,
        tab_title: impl Into<String>,
        pane_id: impl Into<String>,
        pane_title: impl Into<String>,
    ) -> rootcause::Result<Self> {
        let active_tab = TabId::new(tab_id)?;
        let active_pane = PaneId::new(pane_id)?;
        let pane = PaneSnapshot::new(active_pane.clone(), pane_title);
        let tab = TabSnapshot::new(active_tab.clone(), tab_title, active_pane, vec![pane])?;
        Self::new(active_tab, vec![tab])
    }

    /// Validate layout invariants after direct construction or deserialization.
    ///
    /// # Errors
    /// - The active tab, tab list, any tab's active pane, or pane ids are inconsistent.
    pub fn validate(&self) -> rootcause::Result<()> {
        self::validate_layout_id("tab", self.active_tab.as_ref())?;
        if self.tabs.is_empty() {
            return Err(report!("invalid muxr layout snapshot").attach("reason=tabs must not be empty"));
        }
        if !self.tabs.iter().any(|tab| tab.id == self.active_tab) {
            return Err(report!("invalid muxr layout snapshot")
                .attach("reason=active tab is missing")
                .attach(format!("active_tab={}", self.active_tab)));
        }

        let mut seen_tab_ids = Vec::new();
        let mut seen_pane_ids = Vec::new();
        for tab in &self.tabs {
            tab.validate()?;
            if seen_tab_ids.contains(&&tab.id) {
                return Err(report!("invalid muxr layout snapshot")
                    .attach("reason=duplicate tab id")
                    .attach(format!("tab_id={}", tab.id)));
            }
            seen_tab_ids.push(&tab.id);

            for pane in &tab.panes {
                if seen_pane_ids.contains(&&pane.id) {
                    return Err(report!("invalid muxr layout snapshot")
                        .attach("reason=duplicate pane id")
                        .attach(format!("tab_id={}", tab.id))
                        .attach(format!("pane_id={}", pane.id)));
                }
                seen_pane_ids.push(&pane.id);
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TabSnapshot {
    pub active_pane: PaneId,
    pub id: TabId,
    pub panes: Vec<PaneSnapshot>,
    pub title: String,
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

        for (index, pane) in self.panes.iter().enumerate() {
            pane.validate()?;
            if self
                .panes
                .iter()
                .skip(index.saturating_add(1))
                .any(|other_pane| other_pane.id == pane.id)
            {
                return Err(report!("invalid muxr tab snapshot")
                    .attach("reason=duplicate pane id")
                    .attach(format!("tab_id={}", self.id))
                    .attach(format!("pane_id={}", pane.id)));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PaneSnapshot {
    pub id: PaneId,
    pub title: String,
}

impl PaneSnapshot {
    #[must_use]
    pub fn new(id: PaneId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
        }
    }

    fn validate(&self) -> rootcause::Result<()> {
        self::validate_layout_id("pane", self.id.as_ref())
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RenderUpdate {
    Baseline(RenderBaseline),
    Diff(RenderDiff),
}

impl RenderUpdate {
    /// Validate a render update received through public fields or deserialization.
    ///
    /// # Errors
    /// - The baseline or diff violates render protocol invariants.
    pub fn validate(&self) -> rootcause::Result<()> {
        match self {
            Self::Baseline(baseline) => baseline.validate(),
            Self::Diff(diff) => diff.validate(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenderBaseline {
    pub cursor: RenderCursor,
    pub rows: Vec<RenderRowSpan>,
    pub seq: u64,
    pub size: TerminalSize,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenderDiff {
    pub base_seq: u64,
    pub cursor: RenderCursor,
    pub rows: Vec<RenderRowSpan>,
    pub seq: u64,
    pub size: TerminalSize,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenderCursor {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
}

impl RenderCursor {
    #[must_use]
    pub const fn new(row: u16, col: u16, visible: bool) -> Self {
        Self { col, row, visible }
    }

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenderRowSpan {
    pub cells: Vec<RenderCell>,
    pub col: u16,
    pub row: u16,
}

impl RenderRowSpan {
    #[must_use]
    pub const fn new(row: u16, col: u16, cells: Vec<RenderCell>) -> Self {
        Self { cells, col, row }
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
        if self.cells.is_empty() {
            return Err(report!("invalid muxr render row span").attach("reason=cells must not be empty"));
        }
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
        self.validate_wide_cells()?;

        Ok(())
    }

    fn validate_wide_cells(&self) -> rootcause::Result<()> {
        // Deserialized protocol frames can bypass constructors; validate wide-cell pairing before renderers consume it.
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

fn invalid_wide_cell_sequence(reason: &'static str, index: usize) -> rootcause::Report {
    report!("invalid muxr render row span")
        .attach("reason=invalid wide-cell sequence")
        .attach(reason)
        .attach(format!("cell_index={index}"))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenderCell {
    pub style: RenderStyle,
    pub text: String,
    pub width: RenderCellWidth,
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
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RenderCellWidth {
    Narrow,
    Wide,
    WideContinuation,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenderStyle {
    pub attrs: RenderTextStyle,
    pub bg: RenderColor,
    pub fg: RenderColor,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum RenderColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb {
        r: u8,
        g: u8,
        b: u8,
    },
}

/// Normalized key code carried with the original terminal bytes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientKeyModifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
}

impl ClientKeyModifiers {
    pub const ALT: Self = Self::new(true, false, false);
    pub const CTRL_ALT: Self = Self::new(true, true, false);
    pub const NONE: Self = Self::new(false, false, false);
    pub const SHIFT_ALT: Self = Self::new(true, false, true);

    #[must_use]
    pub const fn new(alt: bool, ctrl: bool, shift: bool) -> Self {
        Self { alt, ctrl, shift }
    }
}

/// One ordered keyboard event from the muxr client.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientKey {
    pub code: ClientKeyCode,
    pub modifiers: ClientKeyModifiers,
    pub raw_bytes: Vec<u8>,
}

impl ClientKey {
    #[must_use]
    pub const fn new(code: ClientKeyCode, modifiers: ClientKeyModifiers, raw_bytes: Vec<u8>) -> Self {
        Self {
            code,
            modifiers,
            raw_bytes,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ClientCommand {
    ClosePane,
    CreateTab,
    EnterResizeMode,
    ExitMode,
    FocusPane(PaneFocusDirection),
    FocusNextTab,
    FocusPreviousTab,
    MoveTabNext,
    MoveTabPrevious,
    ResizePane(PaneResizeDirection),
    SplitPaneHorizontal,
    SplitPaneVertical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PaneFocusDirection {
    Down,
    Left,
    Right,
    Up,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PaneResizeDirection {
    Down,
    Left,
    Right,
    Up,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PaneScrollDirection {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientMousePosition {
    pub col: u16,
    pub row: u16,
}

impl ClientMousePosition {
    #[must_use]
    pub const fn new(row: u16, col: u16) -> Self {
        Self { col, row }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "code", content = "msg", rename_all = "snake_case")]
pub enum ServerError {
    ClientAlreadyAttached(String),
    CommandNotImplemented(String),
    ProtocolVersionMismatch(String),
    SessionMismatch(String),
    UnexpectedRequest(String),
}

impl ServerError {
    #[must_use]
    pub fn client_already_attached() -> Self {
        Self::ClientAlreadyAttached("a muxr client is already attached to this session".to_owned())
    }

    #[must_use]
    pub fn command_not_implemented(command: &ClientCommand) -> Self {
        Self::CommandNotImplemented(format!("muxr command is not implemented yet: {command:?}"))
    }

    #[must_use]
    pub fn protocol_version_mismatch(actual: u16) -> Self {
        Self::ProtocolVersionMismatch(format!("expected protocol version {PROTOCOL_VERSION}, got {actual}"))
    }

    #[must_use]
    pub fn session_mismatch(expected: &SessionName, actual: &SessionName) -> Self {
        Self::SessionMismatch(format!("expected session {expected}, got {actual}"))
    }

    #[must_use]
    pub fn unexpected_request(request: &ClientRequest) -> Self {
        Self::UnexpectedRequest(format!("unexpected client request during attach: {request:?}"))
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ClientAlreadyAttached(_) => "client_already_attached",
            Self::CommandNotImplemented(_) => "command_not_implemented",
            Self::ProtocolVersionMismatch(_) => "protocol_version_mismatch",
            Self::SessionMismatch(_) => "session_mismatch",
            Self::UnexpectedRequest(_) => "unexpected_request",
        }
    }

    #[must_use]
    pub const fn is_command_not_implemented(&self) -> bool {
        matches!(self, Self::CommandNotImplemented(_))
    }

    #[must_use]
    pub fn msg(&self) -> &str {
        match self {
            Self::ClientAlreadyAttached(msg)
            | Self::CommandNotImplemented(msg)
            | Self::ProtocolVersionMismatch(msg)
            | Self::SessionMismatch(msg)
            | Self::UnexpectedRequest(msg) => msg,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ClientRequest {
    Hello(ClientHello),
    Ping,
    Pong,
    Detach,
    RenderResync,
    Resize(TerminalSize),
    Input(Vec<u8>),
    Paste(Vec<u8>),
    Key(ClientKey),
    ScrollPaneAt {
        position: ClientMousePosition,
        direction: PaneScrollDirection,
    },
    FocusPaneAt(ClientMousePosition),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ServerEvent {
    Hello(ServerHello),
    Ping,
    Pong,
    Layout(LayoutSnapshot),
    Render(RenderUpdate),
    Output(Vec<u8>),
    Error(ServerError),
    Detached,
}

impl ServerEvent {
    fn validate(&self) -> rootcause::Result<()> {
        // Wire frames and public struct literals can bypass constructors; validate before transport/render use.
        match self {
            Self::Hello(hello) => hello.layout.validate()?,
            Self::Layout(layout) => layout.validate()?,
            Self::Render(update) => update.validate()?,
            Self::Ping | Self::Pong | Self::Output(_) | Self::Error(_) | Self::Detached => {}
        }

        Ok(())
    }
}

/// Encode a client request as a JSON protocol payload.
///
/// # Errors
/// - The request cannot be serialized as JSON.
pub fn encode_client_request(request: &ClientRequest) -> rootcause::Result<Vec<u8>> {
    encode_json_line(request)
}

/// Decode a client request from one JSON protocol payload.
///
/// # Errors
/// - The frame is empty or not a valid client request JSON payload.
pub fn decode_client_request(line: &[u8]) -> rootcause::Result<ClientRequest> {
    decode_json_line(line)
}

/// Encode a server event as a JSON protocol payload.
///
/// # Errors
/// - The event cannot be serialized as JSON.
pub fn encode_server_event(event: &ServerEvent) -> rootcause::Result<Vec<u8>> {
    event.validate()?;
    encode_json_line(event)
}

/// Decode a server event from one JSON protocol payload.
///
/// # Errors
/// - The frame is empty or not a valid server event JSON payload.
pub fn decode_server_event(line: &[u8]) -> rootcause::Result<ServerEvent> {
    let event: ServerEvent = decode_json_line(line)?;
    event.validate()?;
    Ok(event)
}

fn encode_json_line<T>(value: &T) -> rootcause::Result<Vec<u8>>
where
    T: Serialize,
{
    Ok(serde_json::to_vec(value).context("failed to serialize muxr protocol frame")?)
}

fn decode_json_line<T>(line: &[u8]) -> rootcause::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    if line.is_empty() {
        return Err(report!("empty muxr protocol frame"));
    }

    Ok(serde_json::from_slice(line).context("failed to deserialize muxr protocol frame")?)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::hello(ClientRequest::Hello(client_hello()?))]
    #[case::ping(ClientRequest::Ping)]
    #[case::pong(ClientRequest::Pong)]
    #[case::detach(ClientRequest::Detach)]
    #[case::render_resync(ClientRequest::RenderResync)]
    #[case::resize(ClientRequest::Resize(terminal_size(120, 40)?))]
    #[case::input(ClientRequest::Input(vec![b'a', b'b', b'\n']))]
    #[case::paste(ClientRequest::Paste(vec![b'a', b'\n', b'b', b'\n']))]
    #[case::key(ClientRequest::Key(client_key()))]
    #[case::scroll(ClientRequest::ScrollPaneAt {
        position: ClientMousePosition::new(2, 3),
        direction: PaneScrollDirection::Up,
    })]
    #[case::focus_pane_at(ClientRequest::FocusPaneAt(ClientMousePosition::new(2, 3)))]
    fn test_client_request_codec_when_frame_round_trips_returns_original(
        #[case] request: ClientRequest,
    ) -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(decode_client_request(&encode_client_request(&request)?)?, request);
        Ok(())
    }

    #[rstest]
    #[case::hello(ServerEvent::Hello(server_hello()?))]
    #[case::ping(ServerEvent::Ping)]
    #[case::pong(ServerEvent::Pong)]
    #[case::layout(ServerEvent::Layout(layout_snapshot()?))]
    #[case::render_baseline(ServerEvent::Render(RenderUpdate::Baseline(render_baseline()?)))]
    #[case::render_diff(ServerEvent::Render(RenderUpdate::Diff(render_diff()?)))]
    #[case::output(ServerEvent::Output(b"shell output\n".to_vec()))]
    #[case::error(ServerEvent::Error(ServerError::unexpected_request(&ClientRequest::Detach)))]
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
        let encoded = serde_json::to_vec(&event)?;

        assert2::assert!(encode_server_event(&event).is_err());
        assert2::assert!(decode_server_event(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn test_server_event_codec_when_hello_layout_is_invalid_returns_error() -> rootcause::Result<()> {
        let event = ServerEvent::Hello(ServerHello {
            protocol_version: PROTOCOL_VERSION,
            session: "work".parse()?,
            server_pid: ServerPid::new(123)?,
            layout: LayoutSnapshot {
                active_tab: TabId::new("missing")?,
                tabs: vec![tab_snapshot(
                    "tab-1",
                    "default",
                    "pane-1",
                    vec![pane_snapshot("pane-1", "shell")?],
                )?],
            },
        });
        let encoded = serde_json::to_vec(&event)?;

        assert2::assert!(encode_server_event(&event).is_err());
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
        let encoded = serde_json::to_vec(&event)?;

        assert2::assert!(encode_server_event(&event).is_err());
        assert2::assert!(decode_server_event(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn test_server_error_protocol_version_mismatch_returns_structured_error() {
        let error = ServerError::protocol_version_mismatch(1);

        assert2::assert!(
            matches!(error, ServerError::ProtocolVersionMismatch(ref msg) if msg.contains("expected protocol version 9") && msg.contains("got 1"))
        );
    }

    #[test]
    fn test_server_error_command_not_implemented_returns_nonfatal_structured_error() {
        let error = ServerError::command_not_implemented(&ClientCommand::SplitPaneVertical);

        pretty_assertions::assert_eq!(error.code(), "command_not_implemented");
        assert2::assert!(error.is_command_not_implemented());
        assert2::assert!(error.msg().contains("SplitPaneVertical"));
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

    #[test]
    fn test_server_pid_new_when_pid_is_zero_returns_error() {
        assert2::assert!(ServerPid::new(0).is_err());
    }

    #[test]
    fn test_server_pid_codec_when_pid_round_trips_as_number() -> rootcause::Result<()> {
        let encoded = serde_json::to_string(&ServerPid::new(123)?)?;

        pretty_assertions::assert_eq!(encoded, "123");
        pretty_assertions::assert_eq!(serde_json::from_str::<ServerPid>(&encoded)?, ServerPid::new(123)?);
        assert2::assert!(serde_json::from_str::<ServerPid>("0").is_err());
        Ok(())
    }

    #[rstest]
    #[case::zero_seq(0, 80, 24, render_rows(80, 24))]
    #[case::short_rows(1, 80, 24, render_rows(80, 23))]
    #[case::out_of_bounds_row(1, 80, 24, vec![RenderRowSpan::new(24, 0, render_cells(80))])]
    fn test_render_baseline_new_when_frame_is_invalid_returns_error(
        #[case] seq: u64,
        #[case] cols: u16,
        #[case] rows: u16,
        #[case] render_rows: Vec<RenderRowSpan>,
    ) -> rootcause::Result<()> {
        let size = TerminalSize::new(cols, rows)?;
        assert2::assert!(RenderBaseline::new(seq, size, RenderCursor::new(0, 0, true), render_rows).is_err());
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
                RenderCursor::new(0, 0, true),
                vec![RenderRowSpan::new(0, 0, render_cells(1))],
            )
            .is_err()
        );
        Ok(())
    }

    #[rstest]
    #[case::empty_cells(RenderRowSpan::new(0, 0, Vec::new()))]
    #[case::col_out_of_bounds(RenderRowSpan::new(0, 80, render_cells(1)))]
    #[case::span_too_wide(RenderRowSpan::new(0, 79, render_cells(2)))]
    fn test_render_diff_new_when_row_span_is_invalid_returns_error(
        #[case] row: RenderRowSpan,
    ) -> rootcause::Result<()> {
        assert2::assert!(
            RenderDiff::new(
                1,
                2,
                TerminalSize::new(80, 24)?,
                RenderCursor::new(0, 0, true),
                vec![row],
            )
            .is_err()
        );
        Ok(())
    }

    #[rstest]
    #[case::wide_without_continuation(RenderRowSpan::new(
        0,
        0,
        vec![RenderCell::wide("字", RenderStyle::default())]
    ))]
    #[case::continuation_without_wide(RenderRowSpan::new(
        0,
        0,
        vec![RenderCell::wide_continuation(RenderStyle::default())]
    ))]
    #[case::wide_followed_by_narrow(RenderRowSpan::new(
        0,
        0,
        vec![RenderCell::wide("字", RenderStyle::default()), render_cell("x")]
    ))]
    #[case::double_continuation(RenderRowSpan::new(
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
                RenderCursor::new(0, 0, true),
                vec![row],
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_render_baseline_new_when_wide_cell_sequence_is_invalid_returns_error() -> rootcause::Result<()> {
        let rows = vec![RenderRowSpan::new(
            0,
            0,
            vec![RenderCell::wide("字", RenderStyle::default()), render_cell("x")],
        )];

        assert2::assert!(
            RenderBaseline::new(1, TerminalSize::new(2, 1)?, RenderCursor::new(0, 0, true), rows).is_err()
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
        );

        let diff = RenderDiff::new(
            1,
            2,
            TerminalSize::new(80, 24)?,
            RenderCursor::new(0, 0, true),
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
        );

        pretty_assertions::assert_eq!(row.width()?, 2);
        Ok(())
    }

    fn client_hello() -> rootcause::Result<ClientHello> {
        Ok(ClientHello {
            protocol_version: PROTOCOL_VERSION,
            session: "work".parse()?,
            terminal_size: terminal_size(80, 24)?,
        })
    }

    fn server_hello() -> rootcause::Result<ServerHello> {
        Ok(ServerHello {
            protocol_version: PROTOCOL_VERSION,
            session: "work".parse()?,
            server_pid: ServerPid::new(123)?,
            layout: layout_snapshot()?,
        })
    }

    fn client_key() -> ClientKey {
        ClientKey::new(
            ClientKeyCode::Char('E'),
            ClientKeyModifiers::SHIFT_ALT,
            vec![b'\x1b', b'E'],
        )
    }

    fn layout_snapshot() -> rootcause::Result<LayoutSnapshot> {
        LayoutSnapshot::single_pane("tab-1", "default", "pane-1", "shell")
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
        Ok(PaneSnapshot::new(PaneId::new(id)?, title))
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
            RenderCursor::new(1, 2, true),
            vec![
                RenderRowSpan::new(
                    0,
                    0,
                    vec![render_cell("a"), render_cell("b"), render_cell("c"), render_cell("d")],
                ),
                RenderRowSpan::new(
                    1,
                    0,
                    vec![render_cell("e"), render_cell("f"), render_cell("g"), render_cell("h")],
                ),
            ],
        )
    }

    fn render_diff() -> rootcause::Result<RenderDiff> {
        RenderDiff::new(
            1,
            2,
            terminal_size(4, 2)?,
            RenderCursor::new(1, 3, true),
            vec![RenderRowSpan::new(1, 1, vec![render_cell("x"), render_cell("y")])],
        )
    }

    fn invalid_render_event() -> rootcause::Result<ServerEvent> {
        Ok(ServerEvent::Render(RenderUpdate::Diff(RenderDiff {
            base_seq: 1,
            cursor: RenderCursor::new(0, 0, true),
            rows: vec![RenderRowSpan::new(
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
            .map(|row| RenderRowSpan::new(row, 0, render_cells(cols)))
            .collect()
    }

    fn render_cells(cols: u16) -> Vec<RenderCell> {
        (0..cols).map(|_| render_cell(" ")).collect()
    }

    fn render_cell(text: &str) -> RenderCell {
        RenderCell::narrow(text, RenderStyle::default())
    }
}
