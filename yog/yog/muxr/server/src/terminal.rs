use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::PaneMouseMode;
use muxr_core::PaneScrollDirection;
use muxr_core::RenderCell;
use muxr_core::RenderColor;
use muxr_core::RenderCursor;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderTextStyle;
use muxr_core::TerminalSize;
use rootcause::prelude::ResultExt;
use rootcause::report;

// Match the local Zellij scroll buffer so long agent sessions are not truncated sooner in muxr.
const SCROLLBACK_ROWS: usize = 50_000;
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
    screen_dirty_detector: TerminalScreenDirtyDetector,
}

/// Result of feeding PTY bytes into the terminal parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalProcessOutcome {
    replies: Vec<Vec<u8>>,
    screen_dirty: bool,
}

impl TerminalProcessOutcome {
    const fn new(replies: Vec<Vec<u8>>, screen_dirty: bool) -> Self {
        Self { replies, screen_dirty }
    }

    #[must_use]
    pub fn into_replies(self) -> Vec<Vec<u8>> {
        self.replies
    }

    #[must_use]
    pub const fn screen_dirty(&self) -> bool {
        self.screen_dirty
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TerminalScreenDirtyState {
    #[default]
    Ground,
    Escape,
    OscTitleCmd,
    OscTitleSeparator,
    TitleBody,
    TitleBodyEscape,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TerminalScreenDirtyDetector {
    state: TerminalScreenDirtyState,
}

impl TerminalScreenDirtyDetector {
    fn process(&mut self, bytes: &[u8]) -> bool {
        let mut screen_dirty = false;
        for byte in bytes {
            screen_dirty |= self.process_byte(*byte);
        }
        screen_dirty
    }

    const fn process_byte(&mut self, byte: u8) -> bool {
        match self.state {
            TerminalScreenDirtyState::Ground => match byte {
                b'\x1b' => {
                    self.state = TerminalScreenDirtyState::Escape;
                    false
                }
                _ => true,
            },
            TerminalScreenDirtyState::Escape => {
                if byte == b']' {
                    self.state = TerminalScreenDirtyState::OscTitleCmd;
                    false
                } else {
                    self.state = TerminalScreenDirtyState::Ground;
                    true
                }
            }
            TerminalScreenDirtyState::OscTitleCmd => match byte {
                b'0' | b'1' | b'2' => {
                    self.state = TerminalScreenDirtyState::OscTitleSeparator;
                    false
                }
                _ => {
                    self.state = TerminalScreenDirtyState::Ground;
                    true
                }
            },
            TerminalScreenDirtyState::OscTitleSeparator => {
                if byte == b';' {
                    self.state = TerminalScreenDirtyState::TitleBody;
                    false
                } else {
                    self.state = TerminalScreenDirtyState::Ground;
                    true
                }
            }
            TerminalScreenDirtyState::TitleBody => match byte {
                // BEL terminates OSC; `vte` cancels OSC on CAN/SUB so following bytes classify from ground.
                b'\x07' | b'\x18' | b'\x1a' => {
                    self.state = TerminalScreenDirtyState::Ground;
                    false
                }
                b'\x1b' => {
                    self.state = TerminalScreenDirtyState::TitleBodyEscape;
                    false
                }
                _ => false,
            },
            TerminalScreenDirtyState::TitleBodyEscape => {
                self.state = TerminalScreenDirtyState::Ground;
                byte != b'\\'
            }
        }
    }
}

/// Mouse reporting protocol requested by the application running in a pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalMouseProtocol {
    /// Coordinate/button encoding requested by the pane application.
    pub encoding: TerminalMouseProtocolEncoding,
    /// Mouse events requested by the pane application.
    pub mode: TerminalMouseProtocolMode,
}

impl TerminalMouseProtocol {
    pub const fn reports_event(self, event: ClientMouseEvent) -> bool {
        let is_motion = event.button & 32 != 0;
        let is_release = matches!(event.phase, ClientMouseEventPhase::Release);
        match self.mode {
            TerminalMouseProtocolMode::Press => !is_release && !is_motion,
            TerminalMouseProtocolMode::PressRelease => !is_motion,
            // `?1002` button-motion panes must not receive `?1003` hover packets from the outer terminal.
            TerminalMouseProtocolMode::ButtonMotion => !Self::mouse_event_is_no_button_motion(event),
            TerminalMouseProtocolMode::AnyMotion => true,
        }
    }

    pub const fn pane_mouse_mode(self) -> PaneMouseMode {
        match self.mode {
            TerminalMouseProtocolMode::AnyMotion => PaneMouseMode::AnyMotion,
            TerminalMouseProtocolMode::ButtonMotion => PaneMouseMode::ButtonMotion,
            TerminalMouseProtocolMode::Press => PaneMouseMode::Press,
            TerminalMouseProtocolMode::PressRelease => PaneMouseMode::PressRelease,
        }
    }

    const fn mouse_event_is_no_button_motion(event: ClientMouseEvent) -> bool {
        event.button & 32 != 0 && event.button & 0b11 == 0b11
    }
}

/// Terminal modes requested by the application running in a pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalApplicationMode {
    /// Alternate screen is active for a full-screen terminal application.
    pub alternate_screen: bool,
    /// Application cursor mode changes arrow-key escape sequences.
    pub application_cursor: bool,
    /// Mouse reporting protocol requested by the pane application.
    pub mouse_protocol: Option<TerminalMouseProtocol>,
}

impl TerminalApplicationMode {
    pub const fn pane_mouse_mode(self) -> PaneMouseMode {
        match self.mouse_protocol {
            Some(protocol) => protocol.pane_mouse_mode(),
            None => PaneMouseMode::None,
        }
    }
}

/// Mouse event encoding requested by the pane application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalMouseProtocolEncoding {
    /// X10 default byte encoding.
    Default,
    /// SGR `CSI < ... M/m` encoding.
    Sgr,
    /// Deprecated UTF-8 coordinate encoding.
    Utf8,
}

/// Mouse event set requested by the pane application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalMouseProtocolMode {
    /// Report any motion.
    AnyMotion,
    /// Report button motion.
    ButtonMotion,
    /// Report button presses only.
    Press,
    /// Report button presses and releases.
    PressRelease,
}

#[derive(Default)]
struct TerminalCallbacks {
    replies: Vec<Vec<u8>>,
    title: Option<String>,
    title_changes: Vec<Option<String>>,
}

impl TerminalCallbacks {
    fn take_replies(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.replies)
    }

    fn take_title_changes(&mut self) -> Vec<Option<String>> {
        std::mem::take(&mut self.title_changes)
    }

    fn clear_title_metadata(&mut self) {
        self.title = None;
        self.title_changes.clear();
    }
}

impl vt100::Callbacks for TerminalCallbacks {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        first_intermediate: Option<u8>,
        second_intermediate: Option<u8>,
        params: &[&[u16]],
        cmd: char,
    ) {
        if cmd != 'n' || first_intermediate.is_some() || second_intermediate.is_some() {
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

    fn set_window_title(&mut self, _screen: &mut vt100::Screen, title: &[u8]) {
        let title = String::from_utf8_lossy(title).trim().to_owned();
        let title = (!title.is_empty()).then_some(title);
        if self.title != title {
            self.title.clone_from(&title);
            self.title_changes.push(title);
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
            screen_dirty_detector: TerminalScreenDirtyDetector::default(),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) -> TerminalProcessOutcome {
        if bytes.is_empty() {
            return TerminalProcessOutcome::new(Vec::new(), false);
        }

        let screen_dirty = self.screen_dirty_detector.process(bytes);
        // Applications running inside the PTY expect terminal DSR/CPR replies on stdin.
        // `vt100` owns escape parsing, so callbacks preserve split-sequence behavior.
        self.parser.process(bytes);
        TerminalProcessOutcome::new(self.parser.callbacks_mut().take_replies(), screen_dirty)
    }

    pub fn resize(&mut self, size: &TerminalSize) {
        self.parser.screen_mut().set_size(size.rows(), size.cols());
    }

    pub fn title(&self) -> Option<String> {
        self.parser.callbacks().title.clone()
    }

    pub fn take_title_changes(&mut self) -> Vec<Option<String>> {
        self.parser.callbacks_mut().take_title_changes()
    }

    /// Clear OSC title metadata without touching screen contents or scrollback.
    pub fn clear_title_metadata(&mut self) {
        self.parser.callbacks_mut().clear_title_metadata();
    }

    pub fn scroll(&mut self, direction: PaneScrollDirection) -> bool {
        self.scroll_lines(direction, SCROLL_LINES_PER_WHEEL_EVENT)
    }

    pub fn scroll_one_line(&mut self, direction: PaneScrollDirection) -> bool {
        self.scroll_lines(direction, 1)
    }

    fn scroll_lines(&mut self, direction: PaneScrollDirection, lines: usize) -> bool {
        let screen = self.parser.screen_mut();
        let before = screen.scrollback();
        let next = match direction {
            PaneScrollDirection::Down => before.saturating_sub(lines),
            PaneScrollDirection::Up => before.saturating_add(lines),
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

    pub fn visible_top_row(&mut self) -> rootcause::Result<u64> {
        let screen = self.parser.screen_mut();
        let offset = screen.scrollback();
        // vt100 exposes the current viewport offset but not the current scrollback length.
        // Asking it to clamp an oversized scrollback request gives the exact length; restore before returning.
        screen.set_scrollback(usize::MAX);
        let scrollback_len = screen.scrollback();
        screen.set_scrollback(offset);
        let visible_top_row = scrollback_len
            .checked_sub(offset)
            .ok_or_else(|| report!("muxr pane scrollback offset exceeded length"))?;
        Ok(u64::try_from(visible_top_row).context("muxr pane visible top row overflowed")?)
    }

    pub fn bracketed_paste_enabled(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    pub fn application_mode(&self) -> TerminalApplicationMode {
        let screen = self.parser.screen();
        TerminalApplicationMode {
            alternate_screen: screen.alternate_screen(),
            application_cursor: screen.application_cursor(),
            mouse_protocol: self.mouse_protocol(),
        }
    }

    pub fn mouse_protocol(&self) -> Option<TerminalMouseProtocol> {
        let mode = match self.parser.screen().mouse_protocol_mode() {
            vt100::MouseProtocolMode::None => return None,
            vt100::MouseProtocolMode::Press => TerminalMouseProtocolMode::Press,
            vt100::MouseProtocolMode::PressRelease => TerminalMouseProtocolMode::PressRelease,
            vt100::MouseProtocolMode::ButtonMotion => TerminalMouseProtocolMode::ButtonMotion,
            vt100::MouseProtocolMode::AnyMotion => TerminalMouseProtocolMode::AnyMotion,
        };
        let encoding = match self.parser.screen().mouse_protocol_encoding() {
            vt100::MouseProtocolEncoding::Default => TerminalMouseProtocolEncoding::Default,
            vt100::MouseProtocolEncoding::Sgr => TerminalMouseProtocolEncoding::Sgr,
            vt100::MouseProtocolEncoding::Utf8 => TerminalMouseProtocolEncoding::Utf8,
        };
        Some(TerminalMouseProtocol { encoding, mode })
    }

    pub fn snapshot(&self) -> rootcause::Result<TerminalSnapshot> {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let size = TerminalSize::new(cols, rows)?;
        let (cursor_row, cursor_col) = screen.cursor_position();
        let cursor_visible =
            screen.scrollback() == 0 && !screen.hide_cursor() && cursor_row < rows && cursor_col < cols;
        let cursor = RenderCursor {
            row: cursor_row,
            col: cursor_col,
            visible: cursor_visible,
        };
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

        let outcome = terminal.process(b"hi");
        pretty_assertions::assert_eq!(outcome.into_replies(), Vec::<Vec<u8>>::new());
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

        pretty_assertions::assert_eq!(terminal.process(bytes).into_replies(), vec![expected.to_vec()]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_cursor_report_requested_returns_current_cursor() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(b"\x1b[2;3H").into_replies(), Vec::<Vec<u8>>::new());

        pretty_assertions::assert_eq!(terminal.process(b"\x1b[6n").into_replies(), vec![b"\x1b[2;3R".to_vec()]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_report_sequence_is_split_returns_one_reply() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(b"\x1b[").into_replies(), Vec::<Vec<u8>>::new());

        pretty_assertions::assert_eq!(terminal.process(b"6n").into_replies(), vec![b"\x1b[1;1R".to_vec()]);
        Ok(())
    }

    #[rstest]
    #[case::osc_zero(b"\x1b]0;cargo test\x07")]
    #[case::osc_two(b"\x1b]2;cargo test\x07")]
    fn test_terminal_state_title_when_window_title_is_set_returns_title(#[case] bytes: &[u8]) -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(bytes).into_replies(), Vec::<Vec<u8>>::new());

        pretty_assertions::assert_eq!(terminal.title(), Some("cargo test".to_owned()));
        Ok(())
    }

    #[test]
    fn test_terminal_state_take_title_changes_when_window_title_changes_returns_once() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(
            terminal.process(b"\x1b]2;cargo test\x07").into_replies(),
            Vec::<Vec<u8>>::new()
        );

        pretty_assertions::assert_eq!(terminal.take_title_changes(), vec![Some("cargo test".to_owned())]);
        pretty_assertions::assert_eq!(terminal.take_title_changes(), Vec::<Option<String>>::new());
        Ok(())
    }

    #[test]
    fn test_terminal_state_take_title_changes_when_window_title_repeats_returns_empty() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(
            terminal.process(b"\x1b]2;cargo test\x07").into_replies(),
            Vec::<Vec<u8>>::new()
        );
        pretty_assertions::assert_eq!(terminal.take_title_changes(), vec![Some("cargo test".to_owned())]);
        pretty_assertions::assert_eq!(
            terminal.process(b"\x1b]2;cargo test\x07").into_replies(),
            Vec::<Vec<u8>>::new()
        );

        pretty_assertions::assert_eq!(terminal.take_title_changes(), Vec::<Option<String>>::new());
        Ok(())
    }

    #[test]
    fn test_terminal_state_take_title_changes_when_titles_change_in_one_chunk_preserves_order() -> rootcause::Result<()>
    {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(
            terminal.process(b"\x1b]2;gst\x07\x1b]2;~\x07").into_replies(),
            Vec::<Vec<u8>>::new()
        );

        pretty_assertions::assert_eq!(
            terminal.take_title_changes(),
            vec![Some("gst".to_owned()), Some("~".to_owned())],
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_title_when_window_title_sequence_is_split_returns_title() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(b"\x1b]2;").into_replies(), Vec::<Vec<u8>>::new());
        pretty_assertions::assert_eq!(terminal.process(b"gst\x07").into_replies(), Vec::<Vec<u8>>::new());

        pretty_assertions::assert_eq!(terminal.title(), Some("gst".to_owned()));
        Ok(())
    }

    #[test]
    fn test_terminal_state_title_when_window_title_is_empty_returns_none() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        let _ = terminal.process(b"\x1b]2;cargo test\x07");
        let _ = terminal.process(b"\x1b]2;  \x07");

        pretty_assertions::assert_eq!(terminal.title(), None);
        Ok(())
    }

    #[rstest]
    #[case::osc_zero_bel(b"\x1b]0;cargo test\x07")]
    #[case::osc_two_st(b"\x1b]2;cargo test\x1b\\")]
    #[case::multiple_titles(b"\x1b]2;gst\x07\x1b]2;~\x07")]
    fn test_terminal_state_process_when_only_title_changes_keeps_screen_clean(
        #[case] bytes: &[u8],
    ) -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        let outcome = terminal.process(bytes);

        assert2::assert!(!outcome.screen_dirty());
        pretty_assertions::assert_eq!(outcome.into_replies(), Vec::<Vec<u8>>::new());
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_title_sequence_is_split_keeps_screen_clean() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        let first = terminal.process(b"\x1b]2;");
        let second = terminal.process(b"gst\x07");

        assert2::assert!(!first.screen_dirty());
        pretty_assertions::assert_eq!(first.into_replies(), Vec::<Vec<u8>>::new());
        assert2::assert!(!second.screen_dirty());
        pretty_assertions::assert_eq!(second.into_replies(), Vec::<Vec<u8>>::new());
        pretty_assertions::assert_eq!(terminal.title(), Some("gst".to_owned()));
        Ok(())
    }

    #[rstest]
    #[case::text(b"hi")]
    #[case::title_then_text(b"\x1b]2;gst\x07hi")]
    #[case::canceled_title_then_text(b"\x1b]2;gst\x18hi")]
    #[case::unsupported_escape(b"\x1b[6n")]
    fn test_terminal_state_process_when_output_is_not_title_only_marks_screen_dirty(
        #[case] bytes: &[u8],
    ) -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        assert2::assert!(terminal.process(bytes).screen_dirty());
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_output_exceeds_viewport_shows_history() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"one\ntwo\nthree");

        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);
        assert2::assert!(rendered.contains("one"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_to_bottom_when_scrolled_shows_live_viewport() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"one\ntwo\nthree");
        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));

        assert2::assert!(terminal.scroll_to_bottom());
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("three"));
        assert2::assert!(!terminal.scroll_to_bottom());
        Ok(())
    }

    #[test]
    fn test_terminal_state_visible_top_row_when_scrolled_tracks_current_viewport() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"one\ntwo\nthree");
        let bottom_top_row = terminal.visible_top_row()?;
        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let scrolled_snapshot = self::snapshot_text(&terminal.snapshot()?);

        let scrolled_top_row = terminal.visible_top_row()?;

        pretty_assertions::assert_eq!(self::snapshot_text(&terminal.snapshot()?), scrolled_snapshot);
        assert2::assert!(scrolled_top_row < bottom_top_row);
        Ok(())
    }

    #[test]
    fn test_terminal_state_bracketed_paste_when_mode_is_enabled_returns_true() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        let _ = terminal.process(b"\x1b[?2004h");

        assert2::assert!(terminal.bracketed_paste_enabled());
        Ok(())
    }

    #[test]
    fn test_terminal_state_mouse_protocol_when_sgr_button_motion_is_enabled_returns_protocol() -> rootcause::Result<()>
    {
        let mut terminal = TerminalState::new(&terminal_size()?);

        let _ = terminal.process(b"\x1b[?1002h\x1b[?1006h");

        pretty_assertions::assert_eq!(
            terminal.mouse_protocol(),
            Some(TerminalMouseProtocol {
                mode: TerminalMouseProtocolMode::ButtonMotion,
                encoding: TerminalMouseProtocolEncoding::Sgr
            }),
        );
        pretty_assertions::assert_eq!(
            terminal.application_mode().pane_mouse_mode(),
            PaneMouseMode::ButtonMotion
        );
        assert2::assert!(terminal.application_mode().pane_mouse_mode().tracking_enabled());
        Ok(())
    }

    #[rstest]
    #[case::alternate_47_enabled(b"\x1b[?47h", true)]
    #[case::alternate_1049_enabled(b"\x1b[?1049h", true)]
    #[case::alternate_47_disabled(b"\x1b[?47h\x1b[?47l", false)]
    #[case::alternate_1049_disabled(b"\x1b[?1049h\x1b[?1049l", false)]
    fn test_terminal_state_application_mode_when_alternate_screen_changes_returns_state(
        #[case] bytes: &[u8],
        #[case] expected: bool,
    ) -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        let _ = terminal.process(bytes);

        pretty_assertions::assert_eq!(
            terminal.application_mode(),
            TerminalApplicationMode {
                alternate_screen: expected,
                application_cursor: false,
                mouse_protocol: None,
            },
        );
        Ok(())
    }

    #[rstest]
    #[case::application_cursor_enabled(b"\x1b[?1h", true)]
    #[case::application_cursor_disabled(b"\x1b[?1h\x1b[?1l", false)]
    fn test_terminal_state_application_mode_when_application_cursor_changes_returns_state(
        #[case] bytes: &[u8],
        #[case] expected: bool,
    ) -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        let _ = terminal.process(bytes);

        pretty_assertions::assert_eq!(
            terminal.application_mode(),
            TerminalApplicationMode {
                alternate_screen: false,
                application_cursor: expected,
                mouse_protocol: None,
            },
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_application_mode_when_mouse_protocol_is_enabled_returns_protocol() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&terminal_size()?);

        let _ = terminal.process(b"\x1b[?1002h\x1b[?1006h");

        pretty_assertions::assert_eq!(
            terminal.application_mode(),
            TerminalApplicationMode {
                alternate_screen: false,
                application_cursor: false,
                mouse_protocol: Some(TerminalMouseProtocol {
                    mode: TerminalMouseProtocolMode::ButtonMotion,
                    encoding: TerminalMouseProtocolEncoding::Sgr,
                }),
            },
        );
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

        pretty_assertions::assert_eq!(terminal.process(bytes).into_replies(), Vec::<Vec<u8>>::new());
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
