use compact_str::CompactString;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

use super::TerminalSize;

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
        RenderBaseline::new(seq, size, cursor, rows).map_err(super::rkyv_deserialize_error::<D::Error>)
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
        RenderDiff::new(base_seq, seq, size, cursor, rows).map_err(super::rkyv_deserialize_error::<D::Error>)
    }
}

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct RenderCursor {
    pub col: u16,
    pub row: u16,
    pub shape: RenderCursorShape,
    pub visibility: RenderCursorVisibility,
}

impl RenderCursor {
    fn validate(&self, rows: u16, cols: u16) -> rootcause::Result<()> {
        if self.visibility != RenderCursorVisibility::Visible {
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

/// Whether the pane cursor should be rendered.
#[derive(
    rkyv::Archive,
    Clone,
    Copy,
    Debug,
    Default,
    Deserialize,
    rkyv::Deserialize,
    Eq,
    PartialEq,
    Serialize,
    rkyv::Serialize,
)]
pub enum RenderCursorVisibility {
    #[default]
    Hidden,
    Visible,
}

/// Visible cursor shape requested by the pane application.
#[derive(rkyv::Archive, Clone, Copy, Debug, Default, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum RenderCursorShape {
    /// Terminal default cursor shape (`CSI 0 SP q`).
    #[default]
    Default,
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

impl RenderCursorShape {
    #[must_use]
    pub const fn from_csi_param(param: u16) -> Option<Self> {
        match param {
            0 => Some(Self::Default),
            1 => Some(Self::BlinkingBlock),
            2 => Some(Self::SteadyBlock),
            3 => Some(Self::BlinkingUnderline),
            4 => Some(Self::SteadyUnderline),
            5 => Some(Self::BlinkingBar),
            6 => Some(Self::SteadyBar),
            _ => None,
        }
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
        RenderRowSpan::new(row, col, cells).map_err(super::rkyv_deserialize_error::<D::Error>)
    }
}

fn invalid_wide_cell_sequence(reason: &'static str, index: usize) -> rootcause::Report {
    report!("invalid muxr render row span")
        .attach("reason=invalid wide-cell sequence")
        .attach(reason)
        .attach(format!("cell_index={index}"))
}

#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
#[serde(transparent)]
pub struct RenderHyperlink {
    uri: String,
}

impl RenderHyperlink {
    /// Build render hyperlink metadata with a URI safe to emit inside an OSC 8 sequence.
    ///
    /// # Errors
    /// - The URI is empty.
    /// - The URI contains terminal control characters.
    pub fn new(uri: impl Into<String>) -> rootcause::Result<Self> {
        let uri = uri.into();
        if uri.is_empty() {
            return Err(report!("invalid muxr render hyperlink").attach("reason=uri must be nonempty"));
        }
        if uri.chars().any(char::is_control) {
            return Err(report!("invalid muxr render hyperlink").attach("reason=uri must not contain control chars"));
        }

        Ok(Self { uri })
    }

    #[must_use]
    pub fn uri(&self) -> &str {
        &self.uri
    }
}

impl<'de> Deserialize<'de> for RenderHyperlink {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let uri = String::deserialize(deserializer)?;
        Self::new(uri).map_err(serde::de::Error::custom)
    }
}

impl<D> rkyv::Deserialize<RenderHyperlink, D> for ArchivedRenderHyperlink
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<RenderHyperlink, D::Error> {
        let uri = rkyv::Deserialize::<String, D>::deserialize(&self.uri, deserializer)?;
        RenderHyperlink::new(uri).map_err(super::rkyv_deserialize_error::<D::Error>)
    }
}

/// One terminal render cell.
///
/// Cell text is stored compactly because most terminal cells are blank or one glyph; callers stay insulated from that
/// storage choice through text constructors and [`Self::text`].
#[derive(rkyv::Archive, Clone, Debug, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct RenderCell {
    hyperlink: Option<RenderHyperlink>,
    style: RenderStyle,
    text: CompactString,
    width: RenderCellWidth,
}

impl RenderCell {
    #[must_use]
    pub fn narrow(text: impl AsRef<str>, style: RenderStyle) -> Self {
        Self::new(CompactString::new(text.as_ref()), style, RenderCellWidth::Narrow)
    }

    #[must_use]
    pub fn wide(text: impl AsRef<str>, style: RenderStyle) -> Self {
        Self::new(CompactString::new(text.as_ref()), style, RenderCellWidth::Wide)
    }

    #[must_use]
    pub const fn wide_continuation(style: RenderStyle) -> Self {
        Self {
            hyperlink: None,
            style,
            text: CompactString::const_new(""),
            width: RenderCellWidth::WideContinuation,
        }
    }

    #[must_use]
    pub fn with_hyperlink(mut self, hyperlink: RenderHyperlink) -> Self {
        self.hyperlink = Some(hyperlink);
        self
    }

    #[must_use]
    pub const fn with_style(mut self, style: RenderStyle) -> Self {
        self.style = style;
        self
    }

    /// Attach hyperlink metadata to the render cell.
    ///
    /// # Errors
    /// - The URI is invalid for [`RenderHyperlink`].
    pub fn with_hyperlink_uri(self, uri: impl Into<String>) -> rootcause::Result<Self> {
        Ok(self.with_hyperlink(RenderHyperlink::new(uri)?))
    }

    #[must_use]
    pub const fn hyperlink(&self) -> Option<&RenderHyperlink> {
        self.hyperlink.as_ref()
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

    fn with_optional_hyperlink(mut self, hyperlink: Option<RenderHyperlink>) -> Self {
        self.hyperlink = hyperlink;
        self
    }

    const fn new(text: CompactString, style: RenderStyle, width: RenderCellWidth) -> Self {
        Self {
            hyperlink: None,
            style,
            text,
            width,
        }
    }
}

impl<D> rkyv::Deserialize<RenderCell, D> for ArchivedRenderCell
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<RenderCell, D::Error> {
        let hyperlink = rkyv::Deserialize::<Option<RenderHyperlink>, D>::deserialize(&self.hyperlink, deserializer)?;
        let style = rkyv::Deserialize::<RenderStyle, D>::deserialize(&self.style, deserializer)?;
        let text = rkyv::Deserialize::<CompactString, D>::deserialize(&self.text, deserializer)?;
        let width = rkyv::Deserialize::<RenderCellWidth, D>::deserialize(&self.width, deserializer)?;
        match width {
            RenderCellWidth::Narrow => {
                Ok(RenderCell::new(text, style, RenderCellWidth::Narrow).with_optional_hyperlink(hyperlink))
            }
            RenderCellWidth::Wide => {
                Ok(RenderCell::new(text, style, RenderCellWidth::Wide).with_optional_hyperlink(hyperlink))
            }
            RenderCellWidth::WideContinuation => {
                if !text.is_empty() {
                    return Err(super::rkyv_deserialize_error::<D::Error>(
                        "wide continuation cells must not carry text",
                    ));
                }
                Ok(RenderCell::wide_continuation(style).with_optional_hyperlink(hyperlink))
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

#[cfg(test)]
pub mod test_helpers {
    use super::*;

    pub fn raw_render_hyperlink(uri: impl Into<String>) -> RenderHyperlink {
        RenderHyperlink { uri: uri.into() }
    }

    pub fn raw_render_cell(
        hyperlink: Option<RenderHyperlink>,
        style: RenderStyle,
        text: impl AsRef<str>,
        width: RenderCellWidth,
    ) -> RenderCell {
        RenderCell {
            hyperlink,
            style,
            text: CompactString::new(text.as_ref()),
            width,
        }
    }

    pub const fn raw_render_diff(
        base_seq: u64,
        seq: u64,
        size: TerminalSize,
        cursor: RenderCursor,
        rows: Vec<RenderRowSpan>,
    ) -> RenderDiff {
        RenderDiff {
            base_seq,
            cursor,
            rows,
            seq,
            size,
        }
    }

    pub const fn raw_render_row_span(row: u16, col: u16, cells: Vec<RenderCell>) -> RenderRowSpan {
        RenderRowSpan { cells, col, row }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::empty("")]
    #[case::control_char("https://example.com/\u{1b}")]
    fn test_render_hyperlink_new_when_uri_is_invalid_returns_error(#[case] uri: &str) {
        assert2::assert!(RenderHyperlink::new(uri).is_err());
    }

    #[test]
    fn test_render_cell_with_hyperlink_uri_when_uri_is_valid_sets_metadata() -> rootcause::Result<()> {
        let cell = self::render_cell("x").with_hyperlink_uri("https://example.com")?;

        pretty_assertions::assert_eq!(cell.hyperlink().map(RenderHyperlink::uri), Some("https://example.com"));
        assert2::assert!(cell != self::render_cell("x"));
        Ok(())
    }

    #[test]
    fn test_render_cell_with_style_when_cell_has_metadata_preserves_non_style_fields() -> rootcause::Result<()> {
        let original_style = RenderStyle::default();
        let updated_style = RenderStyle {
            attrs: RenderTextStyle::empty().set_dim(true),
            bg: RenderColor::Indexed(1),
            fg: RenderColor::Indexed(2),
        };
        let cell = RenderCell::wide("字", original_style).with_hyperlink_uri("https://example.com")?;

        let updated = cell.with_style(updated_style);

        pretty_assertions::assert_eq!(updated.style(), updated_style);
        pretty_assertions::assert_eq!(updated.text(), "字");
        pretty_assertions::assert_eq!(updated.width(), RenderCellWidth::Wide);
        pretty_assertions::assert_eq!(
            updated.hyperlink().map(RenderHyperlink::uri),
            Some("https://example.com")
        );
        Ok(())
    }

    #[test]
    fn test_render_cell_text_when_serialized_preserves_public_string_shape() -> rootcause::Result<()> {
        let text = "heap-backed render cell text that is longer than compact inline capacity";
        let cell = RenderCell::narrow(text, RenderStyle::default());

        let serialized = serde_json::to_value(&cell)?;

        pretty_assertions::assert_eq!(serialized["text"], serde_json::json!(text));
        Ok(())
    }

    #[test]
    fn test_render_cell_rkyv_deserialize_when_text_is_heap_backed_preserves_cell() -> rootcause::Result<()> {
        let text = "heap-backed render cell text that is longer than compact inline capacity";
        let cell = RenderCell::wide(text, RenderStyle::default()).with_hyperlink_uri("https://example.com")?;
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&cell)?;
        let archived = rkyv::access::<rkyv::Archived<RenderCell>, rkyv::rancor::Error>(&bytes)?;

        let deserialized = rkyv::deserialize::<RenderCell, rkyv::rancor::Error>(archived)?;

        pretty_assertions::assert_eq!(deserialized, cell);
        Ok(())
    }

    #[test]
    fn test_render_cell_rkyv_deserialize_when_hyperlink_uri_is_invalid_returns_error() -> rootcause::Result<()> {
        let cell = test_helpers::raw_render_cell(
            Some(test_helpers::raw_render_hyperlink(String::new())),
            RenderStyle::default(),
            "x",
            RenderCellWidth::Narrow,
        );
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&cell)?;
        let archived = rkyv::access::<rkyv::Archived<RenderCell>, rkyv::rancor::Error>(&bytes)?;

        assert2::assert!(rkyv::deserialize::<RenderCell, rkyv::rancor::Error>(archived).is_err());
        Ok(())
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
                    shape: RenderCursorShape::Default,
                    visibility: RenderCursorVisibility::Visible
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
                    shape: RenderCursorShape::Default,
                    visibility: RenderCursorVisibility::Visible
                },
                vec![RenderRowSpan::new(0, 0, self::render_cells(1))?],
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
                    shape: RenderCursorShape::Default,
                    visibility: RenderCursorVisibility::Visible
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
                    shape: RenderCursorShape::Default,
                    visibility: RenderCursorVisibility::Visible
                },
                vec![row],
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_render_baseline_new_when_wide_cell_sequence_is_invalid_returns_error() -> rootcause::Result<()> {
        let rows = vec![self::raw_render_row_span(
            0,
            0,
            vec![RenderCell::wide("字", RenderStyle::default()), self::render_cell("x")],
        )];

        assert2::assert!(
            RenderBaseline::new(
                1,
                TerminalSize::new(2, 1)?,
                RenderCursor {
                    row: 0,
                    col: 0,
                    shape: RenderCursorShape::Default,
                    visibility: RenderCursorVisibility::Visible
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
                shape: RenderCursorShape::Default,
                visibility: RenderCursorVisibility::Visible,
            },
            vec![row],
        )?;

        pretty_assertions::assert_eq!(diff.rows().len(), 1);
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

    fn render_rows(cols: u16, rows: u16) -> Vec<RenderRowSpan> {
        (0..rows)
            .map(|row| self::raw_render_row_span(row, 0, self::render_cells(cols)))
            .collect()
    }

    fn raw_render_row_span(row: u16, col: u16, cells: Vec<RenderCell>) -> RenderRowSpan {
        test_helpers::raw_render_row_span(row, col, cells)
    }

    fn render_cells(cols: u16) -> Vec<RenderCell> {
        (0..cols).map(|_| self::render_cell(" ")).collect()
    }

    fn render_cell(text: &str) -> RenderCell {
        RenderCell::narrow(text, RenderStyle::default())
    }
}
