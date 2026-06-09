use std::collections::VecDeque;

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
use vte::Params;
use vte::Perform;

// Match the local Zellij scroll buffer so long interactive sessions are not truncated sooner in muxr.
const SCROLLBACK_ROWS: usize = 50_000;
const SCROLL_LINES_PER_WHEEL_EVENT: usize = 5;
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";

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
    partial_scrollback: TerminalPartialScrollback,
}

struct TerminalPartialScrollback {
    parser: vte::Parser,
    rows: VecDeque<Vec<RenderCell>>,
    scroll_region: TerminalScrollRegion,
    size: TerminalSize,
    viewport_offset: usize,
}

impl TerminalPartialScrollback {
    fn new(size: &TerminalSize) -> Self {
        Self {
            parser: vte::Parser::new(),
            rows: VecDeque::new(),
            scroll_region: TerminalScrollRegion::full(size),
            size: size.clone(),
            viewport_offset: 0,
        }
    }

    fn observe_byte(&mut self, byte: u8) -> Option<TerminalPreParserAction> {
        let mut performer = TerminalPartialScrollbackParser {
            action: None,
            scroll_region: self.scroll_region,
            size: &self.size,
        };
        self.parser.advance(&mut performer, &[byte]);
        self.scroll_region = performer.scroll_region;
        performer.action
    }

    fn push_rows(&mut self, rows: impl IntoIterator<Item = Vec<RenderCell>>) {
        for row in rows {
            self.rows.push_back(row);
            while self.rows.len() > SCROLLBACK_ROWS {
                self.rows.pop_front();
                self.viewport_offset = self.viewport_offset.saturating_sub(1);
            }
        }
    }

    const fn captured_rows_for_linefeed_at(&self, cursor_row: u16) -> Option<TerminalScrolledRows> {
        self.scroll_region.captured_rows_for_linefeed_at(cursor_row, &self.size)
    }

    const fn should_capture_linefeed(&self, alternate_screen: bool) -> bool {
        self.scroll_region.should_capture_linefeed(&self.size, alternate_screen)
    }

    fn resize(&mut self, size: &TerminalSize) {
        self.size = size.clone();
        self.scroll_region = self.scroll_region.clamped_to(size);
    }

    fn scroll_to(&mut self, offset: usize, base_scrollback_len: usize) {
        self.viewport_offset = offset.min(self.rows.len().saturating_add(base_scrollback_len));
    }

    fn scroll_by(&mut self, direction: PaneScrollDirection, lines: usize, base_scrollback_len: usize) -> bool {
        let before = self.viewport_offset;
        let total = self.rows.len().saturating_add(base_scrollback_len);
        self.viewport_offset = match direction {
            PaneScrollDirection::Down => self.viewport_offset.saturating_sub(lines),
            PaneScrollDirection::Up => self.viewport_offset.saturating_add(lines).min(total),
        };
        self.viewport_offset != before
    }
}

struct TerminalPartialScrollbackParser<'a> {
    action: Option<TerminalPreParserAction>,
    scroll_region: TerminalScrollRegion,
    size: &'a TerminalSize,
}

impl Perform for TerminalPartialScrollbackParser<'_> {
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore || !intermediates.is_empty() {
            return;
        }

        match action {
            'S' => {
                if let Some(scrolled_rows) = self.scroll_region.captured_rows_for_scroll_up(params, self.size) {
                    self.action = Some(TerminalPreParserAction::CaptureTopRows {
                        count: scrolled_rows.count,
                        full_height: scrolled_rows.full_height,
                    });
                }
            }
            'r' => {
                self.scroll_region = TerminalScrollRegion::from_decstbm(params, self.size);
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalPreParserAction {
    CaptureTopRows { count: usize, full_height: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TerminalScrolledRows {
    count: usize,
    full_height: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TerminalScrollRegion {
    bottom: u16,
    top: u16,
}

impl TerminalScrollRegion {
    const fn full(size: &TerminalSize) -> Self {
        Self {
            bottom: size.rows().saturating_sub(1),
            top: 0,
        }
    }

    fn from_decstbm(params: &Params, size: &TerminalSize) -> Self {
        let primary = self::primary_csi_params(params);
        let top = primary
            .first()
            .copied()
            .filter(|value| *value != 0)
            .unwrap_or(1)
            .saturating_sub(1);
        let bottom = primary
            .get(1)
            .copied()
            .filter(|value| *value != 0)
            .unwrap_or_else(|| size.rows())
            .saturating_sub(1)
            .min(size.rows().saturating_sub(1));
        if top < bottom {
            Self { bottom, top }
        } else {
            Self::full(size)
        }
    }

    fn captured_rows_for_scroll_up(self, params: &Params, size: &TerminalSize) -> Option<TerminalScrolledRows> {
        if self.top != 0 {
            return None;
        }
        let count = self::primary_csi_params(params)
            .first()
            .copied()
            .filter(|value| *value != 0)
            .unwrap_or(1);
        let region_rows = self.bottom.saturating_sub(self.top).saturating_add(1);
        Some(TerminalScrolledRows {
            count: usize::from(count.min(region_rows)),
            full_height: self.bottom >= size.rows().saturating_sub(1),
        })
    }

    const fn captured_rows_for_linefeed_at(self, cursor_row: u16, size: &TerminalSize) -> Option<TerminalScrolledRows> {
        if self.top != 0 || cursor_row != self.bottom {
            return None;
        }
        Some(TerminalScrolledRows {
            count: 1,
            full_height: self.bottom >= size.rows().saturating_sub(1),
        })
    }

    const fn should_capture_linefeed(self, size: &TerminalSize, alternate_screen: bool) -> bool {
        if self.top != 0 {
            return false;
        }
        alternate_screen || self.bottom < size.rows().saturating_sub(1)
    }

    fn clamped_to(self, size: &TerminalSize) -> Self {
        let last_row = size.rows().saturating_sub(1);
        let top = self.top.min(last_row);
        let bottom = self.bottom.min(last_row);
        if top < bottom {
            Self { bottom, top }
        } else {
            Self::full(size)
        }
    }
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
            partial_scrollback: TerminalPartialScrollback::new(size),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) -> TerminalProcessOutcome {
        if bytes.is_empty() {
            return TerminalProcessOutcome::new(Vec::new(), false);
        }

        let screen_dirty = self.screen_dirty_detector.process(bytes);
        let before_scrollback_len = (self.partial_scrollback.viewport_offset > 0).then(|| self.total_scrollback_len());
        // Applications running inside the PTY expect terminal DSR/CPR replies on stdin.
        // `vt100` owns escape parsing, so callbacks preserve split-sequence behavior.
        self.process_with_partial_scrollback_capture(bytes);
        if let Some(before_scrollback_len) = before_scrollback_len {
            let after_scrollback_len = self.total_scrollback_len();
            let added_rows = after_scrollback_len.saturating_sub(before_scrollback_len);
            let base_scrollback_len = self.base_scrollback_len();
            self.partial_scrollback.scroll_to(
                self.partial_scrollback.viewport_offset.saturating_add(added_rows),
                base_scrollback_len,
            );
        }
        self.sync_parser_scrollback();
        TerminalProcessOutcome::new(self.parser.callbacks_mut().take_replies(), screen_dirty)
    }

    pub fn resize(&mut self, size: &TerminalSize) {
        self.parser.screen_mut().set_size(size.rows(), size.cols());
        self.partial_scrollback.resize(size);
        self.sync_parser_scrollback();
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
        let base_scrollback_len = self.base_scrollback_len();
        let changed = self.partial_scrollback.scroll_by(direction, lines, base_scrollback_len);
        self.sync_parser_scrollback();
        changed
    }

    pub fn scroll_to_bottom(&mut self) -> bool {
        let before = self.partial_scrollback.viewport_offset;
        let base_scrollback_len = self.base_scrollback_len();
        self.partial_scrollback.scroll_to(0, base_scrollback_len);
        self.sync_parser_scrollback();
        self.partial_scrollback.viewport_offset != before
    }

    pub fn visible_top_row(&mut self) -> rootcause::Result<u64> {
        let visible_top_row = self
            .total_scrollback_len()
            .checked_sub(self.partial_scrollback.viewport_offset)
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

    pub fn snapshot(&mut self) -> rootcause::Result<TerminalSnapshot> {
        self.sync_parser_scrollback();
        let (screen_rows, screen_cols, cursor_row, cursor_col, hide_cursor) = {
            let screen = self.parser.screen();
            let (rows, cols) = screen.size();
            let (cursor_row, cursor_col) = screen.cursor_position();
            (rows, cols, cursor_row, cursor_col, screen.hide_cursor())
        };
        let size = TerminalSize::new(screen_cols, screen_rows)?;
        let cursor_visible = self.partial_scrollback.viewport_offset == 0
            && !hide_cursor
            && cursor_row < screen_rows
            && cursor_col < screen_cols;
        let cursor = RenderCursor {
            row: cursor_row,
            col: cursor_col,
            visible: cursor_visible,
        };
        let row_cells = self.visible_row_cells(screen_rows);
        let rows = row_cells
            .into_iter()
            .enumerate()
            .map(|(row, cells)| {
                RenderRowSpan::new(
                    u16::try_from(row).context("muxr terminal snapshot row index overflowed")?,
                    0,
                    cells,
                )
            })
            .collect::<rootcause::Result<Vec<_>>>()?;

        Ok(TerminalSnapshot { cursor, rows, size })
    }

    fn base_scrollback_len(&mut self) -> usize {
        let screen = self.parser.screen_mut();
        let offset = screen.scrollback();
        // vt100 exposes the current viewport offset but not the current scrollback length.
        // Asking it to clamp an oversized scrollback request gives the exact length; restore before returning.
        screen.set_scrollback(usize::MAX);
        let scrollback_len = screen.scrollback();
        screen.set_scrollback(offset.min(scrollback_len));
        scrollback_len
    }

    fn capture_top_rows(&mut self, count: usize, full_height: bool) {
        // Codex-style transcript TUIs can paint history with top-starting scroll regions. vt100 already owns
        // normal-screen full-height scrollback, but it drops partial regions and alternate-screen rows.
        if full_height && !self.parser.screen().alternate_screen() {
            return;
        }
        let rows = self.row_cells_at_base_scrollback_offset(0, count);
        self.partial_scrollback.push_rows(rows);
    }

    fn process_with_partial_scrollback_capture(&mut self, bytes: &[u8]) {
        let mut flush_start = 0;
        for (index, byte) in bytes.iter().enumerate() {
            if matches!(*byte, b'\n' | 0x0b | 0x0c)
                && self
                    .partial_scrollback
                    .should_capture_linefeed(self.parser.screen().alternate_screen())
            {
                // Top-starting scroll regions can also drop rows through LF at the bottom
                // boundary. Flush first so vt100's cursor is current before capturing.
                if let Some(pending) = bytes.get(flush_start..index) {
                    self.parser.process(pending);
                }
                flush_start = index;
                let (cursor_row, _) = self.parser.screen().cursor_position();
                if let Some(scrolled_rows) = self.partial_scrollback.captured_rows_for_linefeed_at(cursor_row) {
                    self.capture_top_rows(scrolled_rows.count, scrolled_rows.full_height);
                }
            }
            let Some(action) = self.partial_scrollback.observe_byte(*byte) else {
                continue;
            };
            if let Some(pending) = bytes.get(flush_start..index) {
                self.parser.process(pending);
            }
            match action {
                // Codex scrolls its transcript with a top-starting partial scroll region. vt100 moves those rows out of
                // the visible region without adding them to scrollback, so muxr captures them before feeding the final
                // `S` byte that makes vt100 perform the scroll.
                TerminalPreParserAction::CaptureTopRows { count, full_height } => {
                    self.capture_top_rows(count, full_height);
                }
            }
            flush_start = index;
        }
        if let Some(pending) = bytes.get(flush_start..) {
            self.parser.process(pending);
        }
    }

    fn row_cells_at_base_scrollback_offset(&mut self, offset: usize, row_count: usize) -> Vec<Vec<RenderCell>> {
        let screen = self.parser.screen_mut();
        let previous_offset = screen.scrollback();
        screen.set_scrollback(offset);
        let (rows, cols) = screen.size();
        let row_cells = self::screen_row_cells(screen, rows, cols, row_count);
        screen.set_scrollback(previous_offset);
        row_cells
    }

    fn sync_parser_scrollback(&mut self) {
        let base_offset = self
            .partial_scrollback
            .viewport_offset
            .saturating_sub(self.partial_scrollback.rows.len());
        self.parser.screen_mut().set_scrollback(base_offset);
    }

    fn total_scrollback_len(&mut self) -> usize {
        self.base_scrollback_len()
            .saturating_add(self.partial_scrollback.rows.len())
    }

    fn visible_row_cells(&mut self, screen_rows: u16) -> Vec<Vec<RenderCell>> {
        let height = usize::from(screen_rows);
        let offset = self.partial_scrollback.viewport_offset;
        let captured_len = self.partial_scrollback.rows.len();
        let mut rows = Vec::with_capacity(height);

        if offset == 0 {
            return self.row_cells_at_base_scrollback_offset(0, height);
        }

        if offset <= captured_len {
            let mut captured_rows = self
                .partial_scrollback
                .rows
                .iter()
                .rev()
                .take(offset)
                .cloned()
                .collect::<Vec<_>>();
            captured_rows.reverse();
            rows.extend(captured_rows.into_iter().take(height));
            rows.extend(self.row_cells_at_base_scrollback_offset(0, height.saturating_sub(rows.len())));
            return rows;
        }

        let base_offset = offset.saturating_sub(captured_len);
        let base_rows_in_view = base_offset.min(height);
        rows.extend(self.row_cells_at_base_scrollback_offset(base_offset, base_rows_in_view));
        rows.extend(
            self.partial_scrollback
                .rows
                .iter()
                .take(height.saturating_sub(rows.len()))
                .cloned(),
        );
        rows.extend(self.row_cells_at_base_scrollback_offset(0, height.saturating_sub(rows.len())));
        rows.truncate(height);
        rows
    }
}

pub fn paste_input_bytes(bytes: &[u8], bracketed_paste_enabled: bool) -> Vec<u8> {
    if !bracketed_paste_enabled {
        return bytes.to_vec();
    }

    let mut framed = Vec::with_capacity(
        BRACKETED_PASTE_START
            .len()
            .saturating_add(bytes.len())
            .saturating_add(BRACKETED_PASTE_END.len()),
    );
    framed.extend_from_slice(BRACKETED_PASTE_START);
    framed.extend_from_slice(bytes);
    framed.extend_from_slice(BRACKETED_PASTE_END);
    framed
}

fn single_csi_param(params: &[&[u16]]) -> Option<u16> {
    let param = params.first()?;
    if params.len() != 1 || param.len() != 1 {
        return None;
    }

    param.first().copied()
}

fn primary_csi_params(params: &Params) -> Vec<u16> {
    params.iter().map(|param| param.first().copied().unwrap_or(0)).collect()
}

fn screen_row_cells(screen: &vt100::Screen, rows: u16, cols: u16, row_count: usize) -> Vec<Vec<RenderCell>> {
    (0..rows)
        .take(row_count)
        .map(|row| {
            (0..cols)
                .map(|col| {
                    screen
                        .cell(row, col)
                        .map_or_else(|| RenderCell::narrow(" ", RenderStyle::default()), render_cell)
                })
                .collect()
        })
        .collect()
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
    fn test_paste_input_bytes_when_bracketed_paste_is_enabled_wraps_payload() {
        pretty_assertions::assert_eq!(
            paste_input_bytes(b"one\ntwo\n", true),
            b"\x1b[200~one\ntwo\n\x1b[201~".to_vec(),
        );
    }

    #[test]
    fn test_paste_input_bytes_when_bracketed_paste_is_disabled_preserves_payload() {
        pretty_assertions::assert_eq!(paste_input_bytes(b"one\ntwo\n", false), b"one\ntwo\n".to_vec());
    }

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
    fn test_terminal_state_scroll_when_top_partial_scroll_region_moves_rows_preserves_history() -> rootcause::Result<()>
    {
        let mut terminal = TerminalState::new(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[2S\x1b[r");

        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("one"));
        assert2::assert!(rendered.contains("two"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_partial_scroll_sequence_is_split_preserves_history() -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[");
        let _ = terminal.process(b"2S\x1b[r");

        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("one"));
        assert2::assert!(rendered.contains("two"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_top_partial_scroll_region_linefeed_moves_rows_prefers_captured_history()
    -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"old-0\nold-1\nold-2\nold-3\nold-4\n");
        let _ = terminal.process(b"\x1b[1;1Hcod-0\x1b[2;1Hcod-1\x1b[3;1Hcod-2\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[3;1H\n\x1b[r");

        assert2::assert!(terminal.scroll_one_line(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("cod-0"));
        assert2::assert!(!rendered.contains("old-"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_alternate_screen_partial_scroll_region_moves_rows_preserves_history()
    -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[2S\x1b[r");

        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("one"));
        assert2::assert!(rendered.contains("two"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_alternate_screen_full_scroll_region_moves_rows_preserves_history()
    -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;4r\x1b[2S\x1b[r");

        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("one"));
        assert2::assert!(rendered.contains("two"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_normal_screen_full_scroll_region_moves_rows_preserves_history()
    -> rootcause::Result<()> {
        let mut terminal = TerminalState::new(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;4r\x1b[2S\x1b[r");

        pretty_assertions::assert_eq!(terminal.total_scrollback_len(), 2);
        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("one"));
        assert2::assert!(rendered.contains("two"));
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
