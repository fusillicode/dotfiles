use std::io::Write;

use crossterm::Command;
use crossterm::QueueableCommand;
use crossterm::cursor::Hide;
use crossterm::cursor::MoveTo;
use crossterm::cursor::Show;
use crossterm::style::Attribute;
use crossterm::style::Color;
use crossterm::style::Print;
use crossterm::style::ResetColor;
use crossterm::style::SetAttribute;
use crossterm::style::SetBackgroundColor;
use crossterm::style::SetForegroundColor;
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType;
use muxr_core::RenderCell;
use muxr_core::RenderCellWidth;
use muxr_core::RenderColor;
use muxr_core::RenderCursor;
use muxr_core::RenderDiff;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderTextStyle;
use muxr_core::RenderUpdate;
use muxr_core::TerminalSize;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::copy_selection::SelectionRange;

const OSC8_CLOSE: &[u8] = b"\x1b]8;;\x1b\\";
const OSC8_OPEN_PREFIX: &[u8] = b"\x1b]8;;";
const OSC8_TERMINATOR: &[u8] = b"\x1b\\";

#[derive(Debug, Default)]
pub struct FrameBuffer {
    cursor: Option<RenderCursor>,
    rows: Vec<Vec<RenderCell>>,
    seq: Option<u64>,
    size: Option<TerminalSize>,
}

impl FrameBuffer {
    pub fn apply(&mut self, update: RenderUpdate) -> rootcause::Result<ApplyOutcome> {
        match update {
            RenderUpdate::Baseline(baseline) => {
                let (seq, size, cursor, spans) = baseline.into_parts();
                // RenderBaseline construction validates ordered full-width rows; clone them directly instead of
                // allocating a blank frame and overwriting every cell.
                let rows = spans.iter().map(|span| span.cells().to_vec()).collect();

                self.cursor = Some(cursor.clone());
                self.rows = rows;
                self.seq = Some(seq);
                self.size = Some(size);
                Ok(ApplyOutcome::Applied(RenderFrameChanges {
                    cursor,
                    full_redraw: true,
                    rows: spans,
                }))
            }
            RenderUpdate::Diff(diff) => self.apply_diff(diff),
        }
    }

    fn apply_diff(&mut self, diff: RenderDiff) -> rootcause::Result<ApplyOutcome> {
        let (base_seq, seq, size, cursor, spans) = diff.into_parts();
        if self.seq != Some(base_seq) || self.size.as_ref() != Some(&size) {
            return Ok(ApplyOutcome::NeedsResync);
        }

        // Validate the full diff before mutating so a stale frame cannot leave a partial buffer.
        for row in &spans {
            validate_span_against_rows(&self.rows, row)?;
        }
        for row in &spans {
            apply_span_to_rows(&mut self.rows, row)?;
        }

        self.cursor = Some(cursor.clone());
        self.seq = Some(seq);
        self.size = Some(size);
        Ok(ApplyOutcome::Applied(RenderFrameChanges {
            cursor,
            full_redraw: false,
            rows: spans,
        }))
    }

    pub fn queue_at_with_selection(
        &self,
        stdout: &mut impl Write,
        changes: &RenderFrameChanges,
        row_offset: u16,
        col_offset: u16,
        selection: Option<&SelectionRange>,
        selection_bg: RenderColor,
    ) -> rootcause::Result<()> {
        if self.cursor.as_ref() != Some(&changes.cursor) {
            return Err(report!("muxr render changes do not match current frame buffer cursor"));
        }
        // Diffs still move the real terminal cursor while repainting dirty rows; hide it until the final pane cursor
        // position is restored so intermediate write positions cannot flash across panes.
        queue_cmd(stdout, Hide)?;
        reset_style(stdout)?;
        let mut active_style = RenderStyle::default();
        for row in &changes.rows {
            render_row_span(
                stdout,
                row,
                &mut active_style,
                row_offset,
                col_offset,
                selection,
                selection_bg,
            )?;
        }
        reset_style(stdout)?;
        render_cursor(stdout, &changes.cursor, row_offset, col_offset)?;
        Ok(())
    }

    pub fn row_redraw_changes(&self, changed_rows: &[u16]) -> rootcause::Result<Option<RenderFrameChanges>> {
        let Some(cursor) = self.cursor.clone() else {
            return Ok(None);
        };
        let Some(size) = &self.size else {
            return Ok(None);
        };

        let mut rows = Vec::new();
        for row in changed_rows {
            if *row >= size.rows() {
                continue;
            }
            let Some(cells) = self.rows.get(usize::from(*row)) else {
                return Err(report!("muxr frame buffer row is missing").attach(format!("row={row}")));
            };
            rows.push(RenderRowSpan::new(*row, 0, cells.clone())?);
        }

        Ok(Some(RenderFrameChanges {
            cursor,
            full_redraw: false,
            rows,
        }))
    }

    #[must_use]
    pub fn cell(&self, row: u16, col: u16) -> Option<&RenderCell> {
        self.rows.get(usize::from(row))?.get(usize::from(col))
    }

    #[must_use]
    pub const fn size(&self) -> Option<&TerminalSize> {
        self.size.as_ref()
    }
}

pub fn queue_full_redraw_start(stdout: &mut impl Write) -> rootcause::Result<()> {
    queue_cmd(stdout, Hide)?;
    queue_cmd(stdout, Clear(ClearType::All))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplyOutcome {
    Applied(RenderFrameChanges),
    NeedsResync,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderFrameChanges {
    cursor: RenderCursor,
    full_redraw: bool,
    rows: Vec<RenderRowSpan>,
}

impl RenderFrameChanges {
    #[must_use]
    pub const fn is_full_redraw(&self) -> bool {
        self.full_redraw
    }
}

fn apply_span_to_rows(rows: &mut [Vec<RenderCell>], span: &RenderRowSpan) -> rootcause::Result<()> {
    validate_span_against_rows(rows, span)?;
    let Some(row) = rows.get_mut(usize::from(span.row())) else {
        return Err(report!("muxr render row outside frame").attach(format!("row={}", span.row())));
    };
    let col = usize::from(span.col());

    for (target, cell) in row.iter_mut().skip(col).zip(span.cells().iter()) {
        *target = cell.clone();
    }
    Ok(())
}

fn validate_span_against_rows(rows: &[Vec<RenderCell>], span: &RenderRowSpan) -> rootcause::Result<()> {
    let Some(row) = rows.get(usize::from(span.row())) else {
        return Err(report!("muxr render row outside frame").attach(format!("row={}", span.row())));
    };
    let col = usize::from(span.col());
    let end = col
        .checked_add(span.cells().len())
        .ok_or_else(|| report!("muxr render span column overflowed"))?;
    if end > row.len() {
        return Err(report!("muxr render span outside frame")
            .attach(format!("row={}", span.row()))
            .attach(format!("col={}", span.col()))
            .attach(format!("cells={}", span.cells().len()))
            .attach(format!("cols={}", row.len())));
    }

    Ok(())
}

fn render_row_span(
    stdout: &mut impl Write,
    row: &RenderRowSpan,
    active_style: &mut RenderStyle,
    row_offset: u16,
    col_offset: u16,
    selection: Option<&SelectionRange>,
    selection_bg: RenderColor,
) -> rootcause::Result<()> {
    let rendered_row = row
        .row()
        .checked_add(row_offset)
        .ok_or_else(|| report!("muxr render row offset overflowed"))?;
    let rendered_col = row
        .col()
        .checked_add(col_offset)
        .ok_or_else(|| report!("muxr render column offset overflowed"))?;
    queue_cmd(stdout, MoveTo(rendered_col, rendered_row))?;
    let mut run_style = None;
    let mut run_text = String::new();
    for (index, cell) in row.cells().iter().enumerate() {
        if matches!(cell.width(), RenderCellWidth::WideContinuation) {
            continue;
        }

        let cell_col = row
            .col()
            .checked_add(u16::try_from(index).context("muxr render cell index overflowed")?)
            .ok_or_else(|| report!("muxr render cell column overflowed"))?;
        let cell_style = self::selected_style(cell.style(), selection, row.row(), cell_col, selection_bg);
        let cell_run_style = RenderRunStyle {
            hyperlink_uri: cell.hyperlink().map(muxr_core::RenderHyperlink::uri),
            style: cell_style,
        };
        if run_style != Some(cell_run_style) {
            flush_text_run(stdout, active_style, run_style, &mut run_text)?;
            run_style = Some(cell_run_style);
        }
        if cell.text().is_empty() {
            run_text.push(' ');
        } else {
            run_text.push_str(cell.text());
        }
    }
    flush_text_run(stdout, active_style, run_style, &mut run_text)?;

    Ok(())
}

fn selected_style(
    style: RenderStyle,
    selection: Option<&SelectionRange>,
    row: u16,
    col: u16,
    selection_bg: RenderColor,
) -> RenderStyle {
    self::selection_visual_for_cell(selection, row, col).apply(style, selection_bg)
}

fn selection_visual_for_cell(selection: Option<&SelectionRange>, row: u16, col: u16) -> SelectionVisual {
    if selection.is_some_and(|selection| selection.contains(row, col)) {
        SelectionVisual::Selected
    } else {
        SelectionVisual::Unselected
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectionVisual {
    Selected,
    Unselected,
}

impl SelectionVisual {
    const fn apply(self, mut style: RenderStyle, selection_bg: RenderColor) -> RenderStyle {
        match self {
            Self::Selected => {
                style.attrs = style.attrs.set_inverse(false);
                style.bg = selection_bg;
                style
            }
            Self::Unselected => style,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RenderRunStyle<'a> {
    hyperlink_uri: Option<&'a str>,
    style: RenderStyle,
}

fn flush_text_run(
    stdout: &mut impl Write,
    active_style: &mut RenderStyle,
    run_style: Option<RenderRunStyle<'_>>,
    run_text: &mut String,
) -> rootcause::Result<()> {
    if run_text.is_empty() {
        return Ok(());
    }
    let Some(style) = run_style else {
        return Err(report!("muxr render text run is missing style"));
    };

    apply_style_transition(stdout, active_style, style.style)?;
    if let Some(uri) = style.hyperlink_uri {
        queue_hyperlink_start(stdout, uri)?;
    }
    queue_cmd(stdout, Print(run_text.as_str()))?;
    if style.hyperlink_uri.is_some() {
        queue_hyperlink_end(stdout)?;
    }
    run_text.clear();
    Ok(())
}

fn queue_hyperlink_start(stdout: &mut impl Write, uri: &str) -> rootcause::Result<()> {
    queue_bytes(stdout, OSC8_OPEN_PREFIX)?;
    queue_bytes(stdout, uri.as_bytes())?;
    queue_bytes(stdout, OSC8_TERMINATOR)
}

fn queue_hyperlink_end(stdout: &mut impl Write) -> rootcause::Result<()> {
    queue_bytes(stdout, OSC8_CLOSE)
}

fn apply_style_transition(
    stdout: &mut impl Write,
    active_style: &mut RenderStyle,
    next_style: RenderStyle,
) -> rootcause::Result<()> {
    if *active_style == next_style {
        return Ok(());
    }

    let attrs_changed = active_style.attrs != next_style.attrs;
    if attrs_changed {
        reset_style(stdout)?;
        *active_style = RenderStyle::default();
    }
    if active_style.fg != next_style.fg {
        queue_cmd(stdout, SetForegroundColor(crossterm_color(next_style.fg)))?;
    }
    if active_style.bg != next_style.bg {
        queue_cmd(stdout, SetBackgroundColor(crossterm_color(next_style.bg)))?;
    }
    if attrs_changed {
        apply_enabled_attrs(stdout, next_style.attrs)?;
    }
    *active_style = next_style;
    Ok(())
}

fn reset_style(stdout: &mut impl Write) -> rootcause::Result<()> {
    queue_cmd(stdout, ResetColor)?;
    queue_cmd(stdout, SetAttribute(Attribute::Reset))
}

fn apply_enabled_attrs(stdout: &mut impl Write, attrs: RenderTextStyle) -> rootcause::Result<()> {
    if attrs.bold() {
        queue_cmd(stdout, SetAttribute(Attribute::Bold))?;
    }
    if attrs.dim() {
        queue_cmd(stdout, SetAttribute(Attribute::Dim))?;
    }
    if attrs.italic() {
        queue_cmd(stdout, SetAttribute(Attribute::Italic))?;
    }
    if attrs.underline() {
        queue_cmd(stdout, SetAttribute(Attribute::Underlined))?;
    }
    if attrs.inverse() {
        queue_cmd(stdout, SetAttribute(Attribute::Reverse))?;
    }
    Ok(())
}

fn render_cursor(
    stdout: &mut impl Write,
    cursor: &RenderCursor,
    row_offset: u16,
    col_offset: u16,
) -> rootcause::Result<()> {
    if cursor.visible {
        let row = cursor
            .row
            .checked_add(row_offset)
            .ok_or_else(|| report!("muxr render cursor row offset overflowed"))?;
        let col = cursor
            .col
            .checked_add(col_offset)
            .ok_or_else(|| report!("muxr render cursor column offset overflowed"))?;
        queue_cmd(stdout, MoveTo(col, row))?;
        queue_cmd(stdout, Show)
    } else {
        queue_cmd(stdout, Hide)
    }
}

pub const fn crossterm_color(color: RenderColor) -> Color {
    match color {
        RenderColor::Default => Color::Reset,
        RenderColor::Indexed(index) => Color::AnsiValue(index),
        RenderColor::Rgb { r, g, b } => Color::Rgb { r, g, b },
    }
}

fn queue_cmd<W, C>(stdout: &mut W, cmd: C) -> rootcause::Result<()>
where
    W: Write,
    C: Command,
{
    Ok(stdout
        .queue(cmd)
        .map(|_| ())
        .context("failed to write muxr render frame")?)
}

fn queue_bytes(stdout: &mut impl Write, bytes: &[u8]) -> rootcause::Result<()> {
    stdout
        .write_all(bytes)
        .context("failed to write muxr render escape sequence")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;
    use muxr_core::ClientMousePosition;
    use muxr_core::PaneId;
    use muxr_core::PaneMouseMode;
    use muxr_core::PaneRegionSnapshot;
    use muxr_core::PaneRegionsSnapshot;
    use muxr_core::RenderTextStyle;
    use rstest::rstest;

    use super::*;
    use crate::copy_selection::SelectionInput;
    use crate::copy_selection::SelectionState;

    #[test]
    fn test_frame_buffer_apply_when_baseline_arrives_stores_frame() -> rootcause::Result<()> {
        let mut frame_buffer = FrameBuffer::default();

        let ApplyOutcome::Applied(changes) = frame_buffer.apply(RenderUpdate::Baseline(render_baseline()?))? else {
            return Err(report!("expected applied baseline"));
        };

        assert2::assert!(changes.full_redraw);
        pretty_assertions::assert_eq!(changes.rows.len(), 2);
        pretty_assertions::assert_eq!(frame_buffer.seq, Some(1));
        Ok(())
    }

    #[rstest]
    #[case::missing_baseline(FrameBuffer::default())]
    #[case::wrong_base(applied_frame_buffer()?)]
    fn test_frame_buffer_apply_when_diff_base_is_missing_requests_resync(
        #[case] mut frame_buffer: FrameBuffer,
    ) -> rootcause::Result<()> {
        let outcome = frame_buffer.apply(RenderUpdate::Diff(RenderDiff::new(
            9,
            10,
            terminal_size()?,
            RenderCursor {
                row: 0,
                col: 0,
                visible: true,
            },
            vec![RenderRowSpan::new(0, 0, vec![render_cell("x")])?],
        )?))?;

        pretty_assertions::assert_eq!(outcome, ApplyOutcome::NeedsResync);
        Ok(())
    }

    #[test]
    fn test_frame_buffer_apply_when_diff_arrives_updates_dirty_cells() -> rootcause::Result<()> {
        let mut frame_buffer = applied_frame_buffer()?;

        let ApplyOutcome::Applied(changes) = frame_buffer.apply(RenderUpdate::Diff(render_diff()?))? else {
            return Err(report!("expected applied diff"));
        };

        assert2::assert!(!changes.full_redraw);
        pretty_assertions::assert_eq!(changes.rows.len(), 1);
        pretty_assertions::assert_eq!(frame_buffer.seq, Some(2));
        Ok(())
    }

    #[test]
    fn test_frame_buffer_row_redraw_changes_when_rows_are_supplied_returns_only_requested_rows() -> rootcause::Result<()>
    {
        let frame_buffer = applied_frame_buffer()?;

        let changes = frame_buffer
            .row_redraw_changes(&[1])?
            .ok_or_else(|| report!("expected row redraw changes"))?;

        assert2::assert!(!changes.full_redraw);
        pretty_assertions::assert_eq!(changes.rows.len(), 1);
        pretty_assertions::assert_eq!(changes.rows[0].row(), 1);
        Ok(())
    }

    #[test]
    fn test_frame_buffer_queue_when_changes_arrive_writes_terminal_cmds_without_flushing() -> rootcause::Result<()> {
        let mut frame_buffer = FrameBuffer::default();
        let ApplyOutcome::Applied(changes) = frame_buffer.apply(RenderUpdate::Baseline(render_baseline()?))? else {
            return Err(report!("expected applied baseline"));
        };
        let mut output = CountingWriter::default();

        frame_buffer.queue_at_with_selection(&mut output, &changes, 0, 0, None, MuxrConfig::default().selection.bg)?;

        let rendered = output.rendered_string()?;
        assert2::assert!(rendered.contains('a'));
        assert2::assert!(rendered.contains('d'));
        pretty_assertions::assert_eq!(output.flushes, 0);
        Ok(())
    }

    #[test]
    fn test_frame_buffer_queue_when_diff_arrives_hides_cursor_before_dirty_row_moves() -> rootcause::Result<()> {
        let mut frame_buffer = applied_frame_buffer()?;
        let ApplyOutcome::Applied(changes) = frame_buffer.apply(RenderUpdate::Diff(render_diff()?))? else {
            return Err(report!("expected applied diff"));
        };
        let mut output = Vec::new();

        frame_buffer.queue_at_with_selection(&mut output, &changes, 0, 0, None, MuxrConfig::default().selection.bg)?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        let hide_index = rendered
            .find("\x1b[?25l")
            .ok_or_else(|| report!("expected cursor hide"))?;
        let dirty_row_move_index = rendered
            .find("\x1b[2;2H")
            .ok_or_else(|| report!("expected dirty row cursor move"))?;
        let final_cursor_move_index = rendered
            .rfind("\x1b[2;2H")
            .ok_or_else(|| report!("expected final cursor move"))?;
        assert2::assert!(hide_index < dirty_row_move_index);
        assert2::assert!(dirty_row_move_index < final_cursor_move_index);
        assert2::assert!(rendered.ends_with("\x1b[?25h"));
        Ok(())
    }

    #[test]
    fn test_frame_buffer_queue_at_when_offsets_are_set_offsets_rows_columns_and_cursor() -> rootcause::Result<()> {
        let mut frame_buffer = FrameBuffer::default();
        let ApplyOutcome::Applied(changes) = frame_buffer.apply(RenderUpdate::Baseline(render_baseline()?))? else {
            return Err(report!("expected applied baseline"));
        };
        let mut output = Vec::new();

        frame_buffer.queue_at_with_selection(&mut output, &changes, 1, 2, None, MuxrConfig::default().selection.bg)?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        assert2::assert!(rendered.contains("\x1b[2;3H"));
        assert2::assert!(rendered.contains("\x1b[3;3H"));
        pretty_assertions::assert_eq!(occurrence_count(&rendered, "\x1b[2;3H"), 2);
        Ok(())
    }

    #[test]
    fn test_frame_buffer_render_when_adjacent_cells_share_style_emits_one_color_transition() -> rootcause::Result<()> {
        let style = render_style(RenderColor::Indexed(1), RenderColor::Default, RenderTextStyle::empty());
        let mut frame_buffer = FrameBuffer::default();
        let ApplyOutcome::Applied(changes) =
            frame_buffer.apply(RenderUpdate::Baseline(styled_render_baseline(style)?))?
        else {
            return Err(report!("expected applied baseline"));
        };
        let mut output = Vec::new();

        frame_buffer.queue_at_with_selection(&mut output, &changes, 0, 0, None, MuxrConfig::default().selection.bg)?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        let foreground_escape = expected_escape(ExpectedEscape::Foreground(RenderColor::Indexed(1)))?;
        pretty_assertions::assert_eq!(occurrence_count(&rendered, &foreground_escape), 1);
        assert2::assert!(rendered.contains("abc"));
        Ok(())
    }

    #[test]
    fn test_frame_buffer_render_when_linked_cells_arrive_emits_osc8_around_run() -> rootcause::Result<()> {
        let uri = "https://example.com";
        let mut frame_buffer = FrameBuffer::default();
        let ApplyOutcome::Applied(changes) =
            frame_buffer.apply(RenderUpdate::Baseline(linked_render_baseline(uri)?))?
        else {
            return Err(report!("expected applied baseline"));
        };
        let mut output = Vec::new();

        frame_buffer.queue_at_with_selection(&mut output, &changes, 0, 0, None, MuxrConfig::default().selection.bg)?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        let open = osc8_open(uri);
        let close = osc8_close()?;
        assert2::assert!(rendered.contains(&format!("{open}ab{close}c")));
        pretty_assertions::assert_eq!(occurrence_count(&rendered, &open), 1);
        pretty_assertions::assert_eq!(occurrence_count(&rendered, &close), 1);
        let close_index = rendered.find(&close).ok_or_else(|| report!("expected OSC 8 close"))?;
        let reset_index = rendered
            .rfind("\x1b[0m")
            .ok_or_else(|| report!("expected terminal style reset"))?;
        assert2::assert!(close_index < reset_index);
        Ok(())
    }

    #[test]
    fn test_frame_buffer_render_when_linked_diff_starts_mid_row_emits_osc8_start() -> rootcause::Result<()> {
        let uri = "https://example.com/diff";
        let mut frame_buffer = FrameBuffer::default();
        let ApplyOutcome::Applied(_) = frame_buffer.apply(RenderUpdate::Baseline(render_baseline()?))? else {
            return Err(report!("expected applied baseline"));
        };
        let diff = RenderDiff::new(
            1,
            2,
            terminal_size()?,
            RenderCursor {
                row: 1,
                col: 1,
                visible: true,
            },
            vec![RenderRowSpan::new(1, 1, vec![linked_render_cell("x", uri)?])?],
        )?;
        let ApplyOutcome::Applied(changes) = frame_buffer.apply(RenderUpdate::Diff(diff))? else {
            return Err(report!("expected applied diff"));
        };
        let mut output = Vec::new();

        frame_buffer.queue_at_with_selection(&mut output, &changes, 0, 0, None, MuxrConfig::default().selection.bg)?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        assert2::assert!(rendered.contains(&format!("{}x{}", osc8_open(uri), osc8_close()?)));
        Ok(())
    }

    #[test]
    fn test_frame_buffer_render_when_linked_cell_is_selected_preserves_osc8() -> rootcause::Result<()> {
        let uri = "https://example.com/selected";
        let (selection, _) = self::selection_range_and_style()?;
        let mut frame_buffer = FrameBuffer::default();
        let ApplyOutcome::Applied(changes) =
            frame_buffer.apply(RenderUpdate::Baseline(linked_render_baseline(uri)?))?
        else {
            return Err(report!("expected applied baseline"));
        };
        let mut output = Vec::new();

        frame_buffer.queue_at_with_selection(
            &mut output,
            &changes,
            0,
            0,
            Some(&selection),
            MuxrConfig::default().selection.bg,
        )?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        assert2::assert!(rendered.contains(&osc8_open(uri)));
        Ok(())
    }

    #[test]
    fn test_selection_visual_when_cell_is_selected_marks_only_selected_cells() -> rootcause::Result<()> {
        let (selection, unselected_style) = self::selection_range_and_style()?;
        let selection_bg = MuxrConfig::default().selection.bg;

        pretty_assertions::assert_eq!(
            self::selection_visual_for_cell(Some(&selection), 0, 0),
            SelectionVisual::Selected
        );
        pretty_assertions::assert_eq!(
            self::selection_visual_for_cell(Some(&selection), 0, 2),
            SelectionVisual::Unselected
        );
        pretty_assertions::assert_eq!(self::selection_visual_for_cell(None, 0, 0), SelectionVisual::Unselected);
        let selected_style = self::selected_style(unselected_style, Some(&selection), 0, 0, selection_bg);

        // Selection colors are tunable; this only gates the invariant that selected cells stay visibly distinct.
        assert2::assert!(selected_style.bg != unselected_style.bg);
        assert2::assert!(!selected_style.attrs.inverse());
        pretty_assertions::assert_eq!(
            self::selected_style(unselected_style, Some(&selection), 0, 2, selection_bg),
            unselected_style
        );
        Ok(())
    }

    #[rstest]
    #[case::foreground(
        render_style(RenderColor::Indexed(1), RenderColor::Default, RenderTextStyle::empty()),
        ExpectedEscape::Foreground(RenderColor::Indexed(1))
    )]
    #[case::background(
        render_style(RenderColor::Default, RenderColor::Indexed(2), RenderTextStyle::empty()),
        ExpectedEscape::Background(RenderColor::Indexed(2))
    )]
    #[case::bold(
        render_style(RenderColor::Default, RenderColor::Default, RenderTextStyle::empty().set_bold(true)),
        ExpectedEscape::Attribute(Attribute::Bold)
    )]
    fn test_frame_buffer_render_when_style_changes_emits_expected_transition(
        #[case] style: RenderStyle,
        #[case] expected: ExpectedEscape,
    ) -> rootcause::Result<()> {
        let mut frame_buffer = FrameBuffer::default();
        let ApplyOutcome::Applied(changes) =
            frame_buffer.apply(RenderUpdate::Baseline(styled_render_baseline(style)?))?
        else {
            return Err(report!("expected applied baseline"));
        };
        let mut output = Vec::new();

        frame_buffer.queue_at_with_selection(&mut output, &changes, 0, 0, None, MuxrConfig::default().selection.bg)?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        let expected_escape = expected_escape(expected)?;
        assert2::assert!(rendered.contains(&expected_escape));
        Ok(())
    }

    #[test]
    fn test_queue_full_redraw_start_writes_hide_and_clear_without_flushing() -> rootcause::Result<()> {
        let mut output = CountingWriter::default();

        queue_full_redraw_start(&mut output)?;

        let rendered = output.rendered_string()?;
        assert2::assert!(rendered.contains("\x1b[?25l"));
        assert2::assert!(rendered.contains("\x1b[2J"));
        pretty_assertions::assert_eq!(output.flushes, 0);
        Ok(())
    }

    #[derive(Default)]
    struct CountingWriter {
        bytes: Vec<u8>,
        flushes: usize,
    }

    impl CountingWriter {
        fn rendered_string(&self) -> rootcause::Result<String> {
            Ok(String::from_utf8(self.bytes.clone()).context("muxr render test output was not utf8")?)
        }
    }

    impl std::io::Write for CountingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes = self.flushes.saturating_add(1);
            Ok(())
        }
    }

    fn applied_frame_buffer() -> rootcause::Result<FrameBuffer> {
        let mut frame_buffer = FrameBuffer::default();
        let ApplyOutcome::Applied(_) = frame_buffer.apply(RenderUpdate::Baseline(render_baseline()?))? else {
            return Err(rootcause::report!("expected applied muxr render baseline"));
        };
        Ok(frame_buffer)
    }

    fn render_baseline() -> rootcause::Result<muxr_core::RenderBaseline> {
        muxr_core::RenderBaseline::new(
            1,
            terminal_size()?,
            RenderCursor {
                row: 0,
                col: 0,
                visible: true,
            },
            vec![
                RenderRowSpan::new(0, 0, vec![render_cell("a"), render_cell("b")])?,
                RenderRowSpan::new(1, 0, vec![render_cell("c"), render_cell("d")])?,
            ],
        )
    }

    fn render_diff() -> rootcause::Result<RenderDiff> {
        RenderDiff::new(
            1,
            2,
            terminal_size()?,
            RenderCursor {
                row: 1,
                col: 1,
                visible: true,
            },
            vec![RenderRowSpan::new(1, 1, vec![render_cell("x")])?],
        )
    }

    fn styled_render_baseline(style: RenderStyle) -> rootcause::Result<muxr_core::RenderBaseline> {
        muxr_core::RenderBaseline::new(
            1,
            TerminalSize::new(3, 1)?,
            RenderCursor {
                row: 0,
                col: 0,
                visible: true,
            },
            vec![RenderRowSpan::new(
                0,
                0,
                vec![
                    RenderCell::narrow("a", style),
                    RenderCell::narrow("b", style),
                    RenderCell::narrow("c", style),
                ],
            )?],
        )
    }

    fn linked_render_baseline(uri: &str) -> rootcause::Result<muxr_core::RenderBaseline> {
        muxr_core::RenderBaseline::new(
            1,
            TerminalSize::new(3, 1)?,
            RenderCursor {
                row: 0,
                col: 0,
                visible: true,
            },
            vec![RenderRowSpan::new(
                0,
                0,
                vec![
                    linked_render_cell("a", uri)?,
                    linked_render_cell("b", uri)?,
                    render_cell("c"),
                ],
            )?],
        )
    }

    fn render_style(fg: RenderColor, bg: RenderColor, attrs: RenderTextStyle) -> RenderStyle {
        RenderStyle { attrs, bg, fg }
    }

    fn linked_render_cell(text: &str, uri: &str) -> rootcause::Result<RenderCell> {
        render_cell(text).with_hyperlink_uri(uri)
    }

    fn render_cell(text: &str) -> RenderCell {
        RenderCell::narrow(text, RenderStyle::default())
    }

    fn osc8_open(uri: &str) -> String {
        format!("\x1b]8;;{uri}\x1b\\")
    }

    fn osc8_close() -> rootcause::Result<String> {
        Ok(String::from_utf8(OSC8_CLOSE.to_vec()).context("muxr OSC 8 close was not utf8")?)
    }

    fn terminal_size() -> rootcause::Result<TerminalSize> {
        TerminalSize::new(2, 2)
    }

    fn selection_range_and_style() -> rootcause::Result<(SelectionRange, RenderStyle)> {
        let mut frame_buffer = FrameBuffer::default();
        frame_buffer.apply(RenderUpdate::Baseline(render_baseline()?))?;
        let regions = PaneRegionsSnapshot::new(vec![PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            2,
            1,
            PaneMouseMode::None,
            0,
        )?])?;
        let mut selection = SelectionState::default();
        selection.apply(
            SelectionInput::Start(ClientMousePosition { row: 0, col: 0 }),
            &regions,
            &frame_buffer,
        )?;
        selection.apply(
            SelectionInput::Update(ClientMousePosition { row: 0, col: 1 }),
            &regions,
            &frame_buffer,
        )?;
        let range = selection
            .range()
            .cloned()
            .ok_or_else(|| report!("expected muxr selection range"))?;

        Ok((range, RenderStyle::default()))
    }

    fn occurrence_count(haystack: &str, needle: &str) -> usize {
        haystack.matches(needle).count()
    }

    #[derive(Clone, Copy)]
    enum ExpectedEscape {
        Attribute(Attribute),
        Background(RenderColor),
        Foreground(RenderColor),
    }

    fn expected_escape(expected: ExpectedEscape) -> rootcause::Result<String> {
        let mut output = Vec::new();
        match expected {
            ExpectedEscape::Attribute(attribute) => queue_cmd(&mut output, SetAttribute(attribute))?,
            ExpectedEscape::Background(color) => {
                queue_cmd(&mut output, SetBackgroundColor(crossterm_color(color)))?;
            }
            ExpectedEscape::Foreground(color) => {
                queue_cmd(&mut output, SetForegroundColor(crossterm_color(color)))?;
            }
        }

        Ok(String::from_utf8(output).context("muxr render test output was not utf8")?)
    }
}
