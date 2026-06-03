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
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
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

use crate::client::copy_selection::SelectionRange;

const BRACKETED_PASTE_DISABLE: &[u8] = b"\x1b[?2004l";
const BRACKETED_PASTE_ENABLE: &[u8] = b"\x1b[?2004h";
const MOUSE_BUTTON_CAPTURE_DISABLE: &[u8] = b"\x1b[?1000l";
const MOUSE_BUTTON_CAPTURE_ENABLE: &[u8] = b"\x1b[?1000h";
const MOUSE_BUTTON_EVENT_CAPTURE_DISABLE: &[u8] = b"\x1b[?1002l";
const MOUSE_BUTTON_EVENT_CAPTURE_ENABLE: &[u8] = b"\x1b[?1002h";
const MOUSE_ANY_EVENT_CAPTURE_DISABLE: &[u8] = b"\x1b[?1003l";
const MOUSE_ANY_EVENT_CAPTURE_ENABLE: &[u8] = b"\x1b[?1003h";
const MOUSE_SGR_DISABLE: &[u8] = b"\x1b[?1006l";
const MOUSE_SGR_ENABLE: &[u8] = b"\x1b[?1006h";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SynchronizedOutput {
    Csi,
    Dcs,
}

impl SynchronizedOutput {
    #[must_use]
    pub fn for_term(term: Option<&str>) -> Self {
        match term {
            Some("alacritty") => Self::Dcs,
            Some(_) | None => Self::Csi,
        }
    }

    #[must_use]
    pub const fn start_sequence(self) -> &'static [u8] {
        match self {
            Self::Csi => b"\x1b[?2026h",
            Self::Dcs => b"\x1bP=1s\x1b\\",
        }
    }

    #[must_use]
    pub const fn end_sequence(self) -> &'static [u8] {
        match self {
            Self::Csi => b"\x1b[?2026l",
            Self::Dcs => b"\x1bP=2s\x1b\\",
        }
    }
}

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
    ) -> rootcause::Result<()> {
        if self.cursor.as_ref() != Some(&changes.cursor) {
            return Err(report!("muxr render changes do not match current frame buffer cursor"));
        }
        reset_style(stdout)?;
        let mut active_style = RenderStyle::default();
        for row in &changes.rows {
            render_row_span(stdout, row, &mut active_style, row_offset, col_offset, selection)?;
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

pub fn queue_synchronized_update_start(stdout: &mut impl Write, mode: SynchronizedOutput) -> rootcause::Result<()> {
    stdout
        .write_all(mode.start_sequence())
        .context("failed to write muxr synchronized render start")?;
    Ok(())
}

pub fn queue_synchronized_update_end(stdout: &mut impl Write, mode: SynchronizedOutput) -> rootcause::Result<()> {
    stdout
        .write_all(mode.end_sequence())
        .context("failed to write muxr synchronized render end")?;
    Ok(())
}

/// Enable or disable outer-terminal any-motion mouse capture.
///
/// Pane applications request this mode dynamically. Button-event capture remains enabled, so disabling any-motion
/// returns the client to the lower-volume mouse mode.
///
/// # Errors
/// - The terminal mode sequence cannot be written or flushed.
pub fn set_mouse_any_motion_capture(stdout: &mut impl Write, enabled: bool) -> rootcause::Result<()> {
    if enabled {
        queue_bytes(stdout, MOUSE_ANY_EVENT_CAPTURE_ENABLE)?;
    } else {
        // Some terminals treat mode churn around any-motion capture as a broader mouse-reporting reset. Reassert the
        // button modes muxr owns so pane selection and wheel routing keep working after an app leaves any-motion mode.
        queue_bytes(stdout, MOUSE_ANY_EVENT_CAPTURE_DISABLE)?;
        queue_bytes(stdout, MOUSE_BUTTON_CAPTURE_ENABLE)?;
        queue_bytes(stdout, MOUSE_BUTTON_EVENT_CAPTURE_ENABLE)?;
        queue_bytes(stdout, MOUSE_SGR_ENABLE)?;
    }
    stdout
        .flush()
        .context("failed to flush muxr any-motion mouse capture")?;
    Ok(())
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
        let cell_style = self::selected_style(cell.style(), selection, row.row(), cell_col);
        if run_style != Some(cell_style) {
            flush_text_run(stdout, active_style, run_style, &mut run_text)?;
            run_style = Some(cell_style);
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

fn selected_style(mut style: RenderStyle, selection: Option<&SelectionRange>, row: u16, col: u16) -> RenderStyle {
    if selection.is_some_and(|selection| selection.contains(row, col)) {
        style.attrs = style.attrs.set_inverse(true);
    }
    style
}

fn flush_text_run(
    stdout: &mut impl Write,
    active_style: &mut RenderStyle,
    run_style: Option<RenderStyle>,
    run_text: &mut String,
) -> rootcause::Result<()> {
    if run_text.is_empty() {
        return Ok(());
    }
    let Some(style) = run_style else {
        return Err(report!("muxr render text run is missing style"));
    };

    apply_style_transition(stdout, active_style, style)?;
    queue_cmd(stdout, Print(run_text.as_str()))?;
    run_text.clear();
    Ok(())
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

/// Enter muxr's attached terminal surface.
///
/// The client renders muxr frames on the alternate screen so detach, errors, and final-pane exits cannot leave the muxr
/// screen in the user's outer shell.
///
/// # Errors
/// - The terminal enter cmds cannot be written or flushed.
pub fn enter_terminal(stdout: &mut impl Write) -> rootcause::Result<()> {
    queue_cmd(stdout, EnterAlternateScreen)?;
    queue_bytes(stdout, BRACKETED_PASTE_ENABLE)?;
    // Clear stale any-motion capture; the renderer re-enables it only when a pane requests that mode.
    queue_bytes(stdout, MOUSE_ANY_EVENT_CAPTURE_DISABLE)?;
    queue_bytes(stdout, MOUSE_BUTTON_CAPTURE_ENABLE)?;
    queue_bytes(stdout, MOUSE_BUTTON_EVENT_CAPTURE_ENABLE)?;
    queue_bytes(stdout, MOUSE_SGR_ENABLE)?;
    queue_cmd(stdout, Clear(ClearType::All))?;
    queue_cmd(stdout, Hide)?;
    stdout.flush().context("failed to flush muxr terminal enter")?;
    Ok(())
}

/// Restore terminal render state after muxr exits a rendered session.
///
/// Render frames can hide the cursor and set styles while the client owns the terminal; exit paths call this
/// best-effort cleanup so detach or errors do not leak those modes into the user's shell.
///
/// # Errors
/// - The terminal restore cmds cannot be written or flushed.
pub fn restore_terminal(stdout: &mut impl Write) -> rootcause::Result<()> {
    queue_bytes(stdout, MOUSE_SGR_DISABLE)?;
    queue_bytes(stdout, MOUSE_ANY_EVENT_CAPTURE_DISABLE)?;
    queue_bytes(stdout, MOUSE_BUTTON_EVENT_CAPTURE_DISABLE)?;
    queue_bytes(stdout, MOUSE_BUTTON_CAPTURE_DISABLE)?;
    queue_bytes(stdout, BRACKETED_PASTE_DISABLE)?;
    queue_cmd(stdout, LeaveAlternateScreen)?;
    reset_style(stdout)?;
    queue_cmd(stdout, Show)?;
    stdout.flush().context("failed to flush muxr terminal restore")?;
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

const fn crossterm_color(color: RenderColor) -> Color {
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
        .context("failed to write muxr terminal mode sequence")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use muxr_core::RenderTextStyle;
    use rstest::rstest;

    use super::*;

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

        frame_buffer.queue_at_with_selection(&mut output, &changes, 0, 0, None)?;

        let rendered = output.rendered_string()?;
        assert2::assert!(rendered.contains('a'));
        assert2::assert!(rendered.contains('d'));
        pretty_assertions::assert_eq!(output.flushes, 0);
        Ok(())
    }

    #[test]
    fn test_frame_buffer_queue_at_when_offsets_are_set_offsets_rows_columns_and_cursor() -> rootcause::Result<()> {
        let mut frame_buffer = FrameBuffer::default();
        let ApplyOutcome::Applied(changes) = frame_buffer.apply(RenderUpdate::Baseline(render_baseline()?))? else {
            return Err(report!("expected applied baseline"));
        };
        let mut output = Vec::new();

        frame_buffer.queue_at_with_selection(&mut output, &changes, 1, 2, None)?;

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

        frame_buffer.queue_at_with_selection(&mut output, &changes, 0, 0, None)?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        let foreground_escape = expected_escape(ExpectedEscape::Foreground(RenderColor::Indexed(1)))?;
        pretty_assertions::assert_eq!(occurrence_count(&rendered, &foreground_escape), 1);
        assert2::assert!(rendered.contains("abc"));
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

        frame_buffer.queue_at_with_selection(&mut output, &changes, 0, 0, None)?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        let expected_escape = expected_escape(expected)?;
        assert2::assert!(rendered.contains(&expected_escape));
        Ok(())
    }

    #[test]
    fn test_enter_terminal_writes_alternate_screen_and_clear() -> rootcause::Result<()> {
        let mut output = Vec::new();

        enter_terminal(&mut output)?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        assert2::assert!(rendered.contains("\x1b[?1049h"));
        assert2::assert!(rendered.contains("\x1b[?2004h"));
        assert2::assert!(rendered.contains("\x1b[?1003l"));
        assert2::assert!(rendered.contains("\x1b[?1000h"));
        assert2::assert!(rendered.contains("\x1b[?1002h"));
        assert2::assert!(!rendered.contains("\x1b[?1003h"));
        assert2::assert!(rendered.contains("\x1b[?1006h"));
        assert2::assert!(rendered.contains("\x1b[2J"));
        assert2::assert!(rendered.contains("\x1b[?25l"));
        Ok(())
    }

    #[test]
    fn test_set_mouse_any_motion_capture_when_enabled_writes_any_motion_sequence() -> rootcause::Result<()> {
        let mut output = CountingWriter::default();

        set_mouse_any_motion_capture(&mut output, true)?;

        pretty_assertions::assert_eq!(output.rendered_string()?, "\x1b[?1003h");
        pretty_assertions::assert_eq!(output.flushes, 1);
        Ok(())
    }

    #[test]
    fn test_set_mouse_any_motion_capture_when_disabled_reasserts_button_capture() -> rootcause::Result<()> {
        let mut output = CountingWriter::default();

        set_mouse_any_motion_capture(&mut output, false)?;

        pretty_assertions::assert_eq!(
            output.rendered_string()?,
            "\x1b[?1003l\x1b[?1000h\x1b[?1002h\x1b[?1006h",
        );
        pretty_assertions::assert_eq!(output.flushes, 1);
        Ok(())
    }

    #[rstest]
    #[case::alacritty(Some("alacritty"), SynchronizedOutput::Dcs)]
    #[case::xterm(Some("xterm-256color"), SynchronizedOutput::Csi)]
    #[case::unknown(None, SynchronizedOutput::Csi)]
    fn test_synchronized_output_for_term_when_term_is_known_returns_expected_mode(
        #[case] term: Option<&str>,
        #[case] expected: SynchronizedOutput,
    ) {
        pretty_assertions::assert_eq!(SynchronizedOutput::for_term(term), expected);
    }

    #[rstest]
    #[case::csi(SynchronizedOutput::Csi, "\x1b[?2026h", "\x1b[?2026l")]
    #[case::dcs(SynchronizedOutput::Dcs, "\x1bP=1s\x1b\\", "\x1bP=2s\x1b\\")]
    fn test_synchronized_update_queue_when_mode_is_selected_writes_expected_sequences(
        #[case] mode: SynchronizedOutput,
        #[case] start: &str,
        #[case] end: &str,
    ) -> rootcause::Result<()> {
        let mut output = Vec::new();

        queue_synchronized_update_start(&mut output, mode)?;
        queue_synchronized_update_end(&mut output, mode)?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        pretty_assertions::assert_eq!(rendered, format!("{start}{end}"));
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

    #[test]
    fn test_restore_terminal_writes_alternate_screen_exit_cursor_and_style_reset() -> rootcause::Result<()> {
        let mut output = Vec::new();

        restore_terminal(&mut output)?;

        let rendered = String::from_utf8(output).context("muxr render test output was not utf8")?;
        assert2::assert!(rendered.contains("\x1b[?1006l"));
        assert2::assert!(rendered.contains("\x1b[?1003l"));
        assert2::assert!(rendered.contains("\x1b[?1002l"));
        assert2::assert!(rendered.contains("\x1b[?1000l"));
        assert2::assert!(rendered.contains("\x1b[?2004l"));
        assert2::assert!(rendered.contains("\x1b[?1049l"));
        assert2::assert!(rendered.contains("\x1b[?25h"));
        assert2::assert!(rendered.contains("\x1b[0m"));
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

    fn render_style(fg: RenderColor, bg: RenderColor, attrs: RenderTextStyle) -> RenderStyle {
        RenderStyle { attrs, bg, fg }
    }

    fn render_cell(text: &str) -> RenderCell {
        RenderCell::narrow(text, RenderStyle::default())
    }

    fn terminal_size() -> rootcause::Result<TerminalSize> {
        TerminalSize::new(2, 2)
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
