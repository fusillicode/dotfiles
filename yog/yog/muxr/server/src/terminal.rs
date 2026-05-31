use muxr_core::PaneScrollDirection;
use muxr_core::RenderCell;
use muxr_core::RenderColor;
use muxr_core::RenderCursor;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderTextStyle;
use muxr_core::TerminalSize;

const SCROLLBACK_ROWS: usize = 10_000;
const SCROLL_LINES_PER_WHEEL_EVENT: usize = 5;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalSnapshot {
    cursor: RenderCursor,
    rows: Vec<RenderRowSpan>,
    size: TerminalSize,
}

impl TerminalSnapshot {
    #[must_use]
    pub const fn cursor(&self) -> &RenderCursor {
        &self.cursor
    }

    #[must_use]
    pub fn rows(&self) -> &[RenderRowSpan] {
        &self.rows
    }

    #[must_use]
    pub const fn size(&self) -> &TerminalSize {
        &self.size
    }
}

pub struct TerminalState {
    parser: vt100::Parser<TerminalCallbacks>,
}

#[derive(Default)]
struct TerminalCallbacks {
    replies: Vec<Vec<u8>>,
}

impl TerminalCallbacks {
    fn take_replies(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.replies)
    }
}

impl vt100::Callbacks for TerminalCallbacks {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        first_intermediate: Option<u8>,
        second_intermediate: Option<u8>,
        params: &[&[u16]],
        command: char,
    ) {
        if command != 'n' || first_intermediate.is_some() || second_intermediate.is_some() {
            return;
        }

        match self::single_csi_param(params) {
            Some(5) => self.replies.push(b"\x1b[0n".to_vec()),
            Some(6) => {
                let (row, col) = screen.cursor_position();
                let Some(row) = row.checked_add(1) else {
                    return;
                };
                let Some(col) = col.checked_add(1) else {
                    return;
                };
                self.replies.push(format!("\x1b[{row};{col}R").into_bytes());
            }
            Some(_) | None => {}
        }
    }
}

impl TerminalState {
    pub fn new(size: &TerminalSize) -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(
                size.rows(),
                size.cols(),
                SCROLLBACK_ROWS,
                TerminalCallbacks::default(),
            ),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        if bytes.is_empty() {
            return Vec::new();
        }

        // Applications running inside the PTY expect terminal DSR/CPR replies on stdin.
        // `vt100` owns escape parsing, so callbacks preserve split-sequence behavior.
        self.parser.process(bytes);
        self.parser.callbacks_mut().take_replies()
    }

    pub fn resize(&mut self, size: &TerminalSize) {
        self.parser.screen_mut().set_size(size.rows(), size.cols());
    }

    pub fn scroll(&mut self, direction: PaneScrollDirection) -> bool {
        let screen = self.parser.screen_mut();
        let before = screen.scrollback();
        let next = match direction {
            PaneScrollDirection::Down => before.saturating_sub(SCROLL_LINES_PER_WHEEL_EVENT),
            PaneScrollDirection::Up => before.saturating_add(SCROLL_LINES_PER_WHEEL_EVENT),
        };
        screen.set_scrollback(next);
        screen.scrollback() != before
    }

    pub fn scroll_to_bottom(&mut self) -> bool {
        let screen = self.parser.screen_mut();
        let before = screen.scrollback();
        screen.set_scrollback(0);
        screen.scrollback() != before
    }

    pub fn bracketed_paste_enabled(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    pub fn snapshot(&self) -> rootcause::Result<TerminalSnapshot> {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let size = TerminalSize::new(cols, rows)?;
        let (cursor_row, cursor_col) = screen.cursor_position();
        let cursor_visible =
            screen.scrollback() == 0 && !screen.hide_cursor() && cursor_row < rows && cursor_col < cols;
        let cursor = RenderCursor::new(cursor_row, cursor_col, cursor_visible);
        let rows = (0..rows)
            .map(|row| {
                let cells = (0..cols)
                    .map(|col| {
                        screen
                            .cell(row, col)
                            .map_or_else(|| RenderCell::narrow(" ", RenderStyle::default()), render_cell)
                    })
                    .collect();
                RenderRowSpan::new(row, 0, cells)
            })
            .collect::<rootcause::Result<Vec<_>>>()?;

        Ok(TerminalSnapshot { cursor, rows, size })
    }
}

fn single_csi_param(params: &[&[u16]]) -> Option<u16> {
    let param = params.first()?;
    if params.len() != 1 || param.len() != 1 {
        return None;
    }

    param.first().copied()
}

fn render_cell(cell: &vt100::Cell) -> RenderCell {
    let style = render_style(cell);
    if cell.is_wide_continuation() {
        return RenderCell::wide_continuation(style);
    }

    let text = if cell.has_contents() { cell.contents() } else { " " };
    if cell.is_wide() {
        RenderCell::wide(text, style)
    } else {
        RenderCell::narrow(text, style)
    }
}

fn render_style(cell: &vt100::Cell) -> RenderStyle {
    RenderStyle {
        attrs: RenderTextStyle::empty()
            .set_bold(cell.bold())
            .set_dim(cell.dim())
            .set_italic(cell.italic())
            .set_underline(cell.underline())
            .set_inverse(cell.inverse()),
        bg: render_color(cell.bgcolor()),
        fg: render_color(cell.fgcolor()),
    }
}

const fn render_color(color: vt100::Color) -> RenderColor {
    match color {
        vt100::Color::Default => RenderColor::Default,
        vt100::Color::Idx(index) => RenderColor::Indexed(index),
        vt100::Color::Rgb(r, g, b) => RenderColor::Rgb { r, g, b },
    }
}

#[cfg(test)]
mod tests {
    use rootcause::report;
    use rstest::rstest;

    use super::*;

    #[test]
    fn test_terminal_state_snapshot_when_output_processed_contains_screen() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(b"hi"), Vec::<Vec<u8>>::new());
        let snapshot = terminal.snapshot()?;
        let Some(row) = snapshot.rows().first() else {
            return Err(report!("expected first render row"));
        };
        let rendered = row.cells().iter().take(2).map(RenderCell::text).collect::<String>();

        pretty_assertions::assert_eq!(rendered, "hi");
        Ok(())
    }

    #[rstest]
    #[case::status_report(b"\x1b[5n", b"\x1b[0n")]
    #[case::cursor_report(b"\x1b[6n", b"\x1b[1;1R")]
    fn test_terminal_state_process_when_terminal_report_requested_returns_reply(
        #[case] bytes: &[u8],
        #[case] expected: &[u8],
    ) -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(bytes), vec![expected.to_vec()]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_cursor_report_requested_returns_current_cursor() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(b"\x1b[2;3H"), Vec::<Vec<u8>>::new());

        pretty_assertions::assert_eq!(terminal.process(b"\x1b[6n"), vec![b"\x1b[2;3R".to_vec()]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_report_sequence_is_split_returns_one_reply() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(b"\x1b["), Vec::<Vec<u8>>::new());

        pretty_assertions::assert_eq!(terminal.process(b"6n"), vec![b"\x1b[1;1R".to_vec()]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_output_exceeds_viewport_shows_history() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&TerminalSize::new(8, 2)?);

        terminal.process(b"one\ntwo\nthree");

        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);
        assert2::assert!(rendered.contains("one"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_to_bottom_when_scrolled_shows_live_viewport() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&TerminalSize::new(8, 2)?);

        terminal.process(b"one\ntwo\nthree");
        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));

        assert2::assert!(terminal.scroll_to_bottom());
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("three"));
        assert2::assert!(!terminal.scroll_to_bottom());
        Ok(())
    }

    #[test]
    fn test_terminal_state_bracketed_paste_when_mode_is_enabled_returns_true() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        terminal.process(b"\x1b[?2004h");

        assert2::assert!(terminal.bracketed_paste_enabled());
        Ok(())
    }

    #[rstest]
    #[case::private_cursor_report(b"\x1b[?6n")]
    #[case::unknown_report(b"\x1b[9n")]
    #[case::multi_param_report(b"\x1b[5;6n")]
    fn test_terminal_state_process_when_report_is_unsupported_returns_no_reply(
        #[case] bytes: &[u8],
    ) -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(bytes), Vec::<Vec<u8>>::new());
        Ok(())
    }

    fn terminal_size() -> rootcause::Result<TerminalSize> {
        TerminalSize::new(8, 4)
    }

    fn snapshot_text(snapshot: &TerminalSnapshot) -> String {
        snapshot
            .rows()
            .iter()
            .flat_map(|row| row.cells().iter().map(RenderCell::text))
            .collect()
    }
}
