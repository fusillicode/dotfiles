use std::io::Write;

use muxr_config::ScrollbackConfig;
use muxr_config::ScrollbackDumpStyle;
use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::PaneMouseMode;
use muxr_core::PaneScrollDirection;
use muxr_core::RenderCell;
use muxr_core::RenderCellWidth;
use muxr_core::RenderColor;
use muxr_core::RenderCursor;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderTextStyle;
use muxr_core::TerminalSize;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::terminal_scrollback::TerminalPreParserAction;
use crate::terminal_scrollback::TerminalScrollback;

const SCROLL_LINES_PER_WHEEL_EVENT: usize = 5;
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const FOCUS_REPORTING_MODE: u16 = 1004;
const KITTY_KEYBOARD_PROTOCOL_DISABLED_REPLY: &[u8] = b"\x1b[?0u";
const KITTY_KEYBOARD_PROTOCOL_DISAMBIGUATE_ESC_CODES_MODE: u16 = 1;
const KITTY_KEYBOARD_PROTOCOL_ENABLED_REPLY: &[u8] = b"\x1b[?1u";
const KITTY_KEYBOARD_PROTOCOL_SET_DIFFERENCE: u16 = 3;
const REPLAY_ALTERNATE_SCREEN_EXIT_BYTES: &[u8] = b"\x1b[?1049l";
const REPLAY_APPLICATION_STATE_RESET_BYTES: &[u8] =
    b"\x18\x1b>\x1b[?1l\x1b[?6l\x1b[?9l\x1b[?47l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?2004l\x1b[?25h\x1b[r";

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
    reset_detector: TerminalResetDetector,
    screen_dirty_detector: TerminalScreenDirtyDetector,
    scrollback: TerminalScrollback,
}

#[derive(Default)]
struct TerminalResetDetector {
    pending_escape: bool,
}

impl TerminalResetDetector {
    const fn observe_byte(&mut self, byte: u8) -> bool {
        if self.pending_escape {
            if byte == b'c' {
                self.pending_escape = false;
                return true;
            }
            self.pending_escape = byte == 0x1b;
        } else if byte == 0x1b {
            self.pending_escape = true;
        }
        false
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
    pub screen_mode: TerminalScreenMode,
    /// Application cursor mode changes arrow-key escape sequences.
    pub cursor_key_mode: TerminalCursorKeyMode,
    /// Keyboard protocol requested by the pane application.
    pub keyboard_protocol: TerminalKeyboardProtocol,
    /// Focus reporting forwards muxr pane/tab focus changes to applications that enabled `CSI ? 1004 h`.
    pub focus_reporting: TerminalFocusReporting,
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

/// Keyboard encoding requested by the pane application.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalKeyboardProtocol {
    #[default]
    Legacy,
    KittyLevelOne,
}

impl TerminalKeyboardProtocol {
    #[must_use]
    const fn reply_bytes(self) -> &'static [u8] {
        match self {
            Self::Legacy => KITTY_KEYBOARD_PROTOCOL_DISABLED_REPLY,
            Self::KittyLevelOne => KITTY_KEYBOARD_PROTOCOL_ENABLED_REPLY,
        }
    }
}

const fn keyboard_protocol_from_mode(mode: u16) -> TerminalKeyboardProtocol {
    if mode & KITTY_KEYBOARD_PROTOCOL_DISAMBIGUATE_ESC_CODES_MODE == 0 {
        TerminalKeyboardProtocol::Legacy
    } else {
        TerminalKeyboardProtocol::KittyLevelOne
    }
}

fn keyboard_protocol_from_params(params: &[&[u16]]) -> TerminalKeyboardProtocol {
    let mode = params.first().and_then(|param| param.first()).copied().unwrap_or(0);
    self::keyboard_protocol_from_mode(mode)
}

/// Terminal screen buffer selected by the pane application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalScreenMode {
    Alternate,
    Normal,
}

impl TerminalScreenMode {
    #[must_use]
    const fn from_alternate_screen(alternate_screen: bool) -> Self {
        if alternate_screen {
            Self::Alternate
        } else {
            Self::Normal
        }
    }
}

/// Cursor-key escape sequence mode selected by the pane application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalCursorKeyMode {
    Application,
    Normal,
}

impl TerminalCursorKeyMode {
    #[must_use]
    const fn from_application_cursor(application_cursor: bool) -> Self {
        if application_cursor {
            Self::Application
        } else {
            Self::Normal
        }
    }
}

/// Focus reporting mode selected by the pane application.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalFocusReporting {
    #[default]
    Disabled,
    Enabled,
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

/// Terminal focus event forwarded to applications that requested focus reporting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalFocusEvent {
    Gained,
    Lost,
}

impl TerminalFocusEvent {
    #[must_use]
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Gained => b"\x1b[I",
            Self::Lost => b"\x1b[O",
        }
    }
}

#[derive(Default)]
struct TerminalCallbacks {
    focus_reporting: TerminalFocusReporting,
    // muxr only needs the active legacy/level-one decision to downgrade keys at the pane boundary.
    // Full kitty push/pop and set-behavior semantics stay out of this MVP until callers need them.
    keyboard_protocol: TerminalKeyboardProtocol,
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

    const fn clear_tracked_application_modes(&mut self) {
        self.focus_reporting = TerminalFocusReporting::Disabled;
        self.keyboard_protocol = TerminalKeyboardProtocol::Legacy;
    }

    fn update_focus_reporting(&mut self, params: &[&[u16]], enabled: bool) {
        if params.iter().any(|param| *param == [FOCUS_REPORTING_MODE]) {
            self.focus_reporting = if enabled {
                TerminalFocusReporting::Enabled
            } else {
                TerminalFocusReporting::Disabled
            };
        }
    }

    fn update_keyboard_protocol_level(&mut self, params: &[&[u16]]) {
        self.keyboard_protocol = self::keyboard_protocol_from_params(params);
    }

    fn set_keyboard_protocol_level(&mut self, params: &[&[u16]]) {
        let requested = self::keyboard_protocol_from_params(params);
        let behavior = params.get(1).and_then(|param| param.first()).copied();
        if behavior == Some(KITTY_KEYBOARD_PROTOCOL_SET_DIFFERENCE) {
            // Do not reintroduce kitty's full stack/set model; this just prevents a one-bit opt-out from being
            // misread as an enable and leaking CSI-u bytes after the pane removed level-one support.
            if requested == TerminalKeyboardProtocol::KittyLevelOne {
                self.keyboard_protocol = TerminalKeyboardProtocol::Legacy;
            }
        } else {
            self.keyboard_protocol = requested;
        }
    }

    fn push_keyboard_protocol_reply(&mut self) {
        self.replies.push(self.keyboard_protocol.reply_bytes().to_vec());
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
        match (first_intermediate, second_intermediate, cmd) {
            (Some(b'?'), None, 'h') => self.update_focus_reporting(params, true),
            (Some(b'?'), None, 'l') => self.update_focus_reporting(params, false),
            (Some(b'>'), None, 'u') => self.update_keyboard_protocol_level(params),
            (Some(b'='), None, 'u') => self.set_keyboard_protocol_level(params),
            (Some(b'<'), None, 'u') => self.keyboard_protocol = TerminalKeyboardProtocol::Legacy,
            (Some(b'?'), None, 'u') => self.push_keyboard_protocol_reply(),
            _ => {}
        }

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
    pub fn with_scrollback(size: &TerminalSize, scrollback: ScrollbackConfig) -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(size.rows(), size.cols(), 0, TerminalCallbacks::default()),
            reset_detector: TerminalResetDetector::default(),
            screen_dirty_detector: TerminalScreenDirtyDetector::default(),
            scrollback: TerminalScrollback::new(size, scrollback.rows),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) -> TerminalProcessOutcome {
        if bytes.is_empty() {
            return TerminalProcessOutcome::new(Vec::new(), false);
        }

        let screen_dirty = self.screen_dirty_detector.process(bytes);
        let before_scrollback_len = (self.scrollback.viewport_offset() > 0).then(|| self.total_scrollback_len());
        // Applications running inside the PTY expect terminal DSR/CPR replies on stdin.
        // `vt100` owns escape parsing, so callbacks preserve split-sequence behavior.
        self.process_with_scrollback_capture(bytes);
        if let Some(before_scrollback_len) = before_scrollback_len {
            let after_scrollback_len = self.total_scrollback_len();
            let added_rows = after_scrollback_len.saturating_sub(before_scrollback_len);
            self.scrollback
                .scroll_to(self.scrollback.viewport_offset().saturating_add(added_rows));
        }
        TerminalProcessOutcome::new(self.parser.callbacks_mut().take_replies(), screen_dirty)
    }

    pub fn resize(&mut self, size: &TerminalSize) {
        self.parser.screen_mut().set_size(size.rows(), size.cols());
        self.scrollback.resize(size);
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

    /// Clear app-owned terminal modes that must come only from the live PTY process, not replayed history.
    pub fn clear_replayed_application_state(&mut self) {
        if self.parser.screen().alternate_screen() {
            self.process_with_scrollback_capture(REPLAY_ALTERNATE_SCREEN_EXIT_BYTES);
        }
        // Raw history replay may start or end inside a full-screen app. Keep replayed cells/scrollback, then return
        // modes to shell defaults so later normal output is captured as scrollback.
        self.process_with_scrollback_capture(REPLAY_APPLICATION_STATE_RESET_BYTES);
        let _replies = self.parser.callbacks_mut().take_replies();
        self.parser.callbacks_mut().clear_tracked_application_modes();
        self.scrollback.clear_replayed_application_state();
    }

    pub fn scroll(&mut self, direction: PaneScrollDirection) -> bool {
        self.scroll_lines(direction, SCROLL_LINES_PER_WHEEL_EVENT)
    }

    pub fn scroll_one_line(&mut self, direction: PaneScrollDirection) -> bool {
        self.scroll_lines(direction, 1)
    }

    fn scroll_lines(&mut self, direction: PaneScrollDirection, lines: usize) -> bool {
        self.scrollback.scroll_by(direction, lines)
    }

    pub fn scroll_to_bottom(&mut self) -> bool {
        let before = self.scrollback.viewport_offset();
        self.scrollback.scroll_to(0);
        self.scrollback.viewport_offset() != before
    }

    pub fn visible_top_row(&self) -> rootcause::Result<u64> {
        let visible_top_row = self
            .total_scrollback_len()
            .checked_sub(self.scrollback.viewport_offset())
            .ok_or_else(|| report!("muxr pane scrollback offset exceeded length"))?;
        Ok(u64::try_from(visible_top_row).context("muxr pane visible top row overflowed")?)
    }

    pub fn bracketed_paste_enabled(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    pub fn application_mode(&self) -> TerminalApplicationMode {
        let screen = self.parser.screen();
        TerminalApplicationMode {
            screen_mode: TerminalScreenMode::from_alternate_screen(screen.alternate_screen()),
            cursor_key_mode: TerminalCursorKeyMode::from_application_cursor(screen.application_cursor()),
            keyboard_protocol: self.parser.callbacks().keyboard_protocol,
            focus_reporting: self.parser.callbacks().focus_reporting,
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
        let (screen_rows, screen_cols, cursor_row, cursor_col, hide_cursor) = {
            let screen = self.parser.screen();
            let (rows, cols) = screen.size();
            let (cursor_row, cursor_col) = screen.cursor_position();
            (rows, cols, cursor_row, cursor_col, screen.hide_cursor())
        };
        let size = TerminalSize::new(screen_cols, screen_rows)?;
        let cursor_visible = self.scrollback.viewport_offset() == 0
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

    pub fn scrollback_dump(&self, style: ScrollbackDumpStyle) -> std::io::Result<Vec<u8>> {
        let mut dump = Vec::new();
        let (screen_rows, _screen_cols) = self.parser.screen().size();
        // Dump order mirrors what a user expects to read: muxr-owned scrollback first, then the live bottom viewport.
        for row in self
            .scrollback
            .captured_oldest_row_cells_iter(self.scrollback.captured_len())
        {
            self::write_scrollback_dump_row(&row, style, &mut dump)?;
        }
        self::write_scrollback_dump_rows(&self.live_row_cells(usize::from(screen_rows)), style, &mut dump)?;
        Ok(dump)
    }

    fn capture_top_rows(&mut self, count: usize) {
        // muxr keeps one ordered scrollback stream. If vt100 also retained full-height history, partial-scroll-region
        // rows and normal rows would live in separate stores and later normal output could appear older than earlier
        // partial-region output while scrolling.
        if self.parser.screen().alternate_screen() {
            return;
        }
        let rows = self.live_row_cells(count);
        self.scrollback.push_rows(rows);
    }

    fn process_with_scrollback_capture(&mut self, bytes: &[u8]) {
        let mut flush_start = 0;
        let mut pending_print_width = 0_usize;
        for (index, byte) in bytes.iter().enumerate() {
            if matches!(*byte, b'\n' | 0x0b | 0x0c)
                && self
                    .scrollback
                    .should_capture_linefeed(self.parser.screen().alternate_screen())
            {
                // Top-starting scroll regions can also drop rows through LF at the bottom
                // boundary. Flush first so vt100's cursor is current before capturing.
                if let Some(pending) = bytes.get(flush_start..index) {
                    self.parser.process(pending);
                }
                pending_print_width = 0;
                flush_start = index;
                let (cursor_row, _) = self.parser.screen().cursor_position();
                if let Some(count) = self.scrollback.captured_rows_for_linefeed_at(cursor_row) {
                    self.capture_top_rows(count);
                }
            }
            let reset_here = self.reset_detector.observe_byte(*byte);
            if let Some(action) = self.scrollback.observe_byte(*byte) {
                match action {
                    TerminalPreParserAction::Printable { width } => {
                        let (cursor_row, cursor_col) = self.parser.screen().cursor_position();
                        if self.scrollback.autowrap_capture_possible_after_prints(
                            cursor_row,
                            cursor_col,
                            pending_print_width,
                            width,
                        ) {
                            if let Some(pending) = bytes.get(flush_start..index) {
                                self.parser.process(pending);
                            }
                            let (cursor_row, cursor_col) = self.parser.screen().cursor_position();
                            if let Some(count) = self
                                .scrollback
                                .captured_rows_for_autowrap_at(cursor_row, cursor_col, width)
                            {
                                self.capture_top_rows(count);
                            }
                            pending_print_width = usize::from(width);
                            flush_start = index;
                        } else {
                            pending_print_width = pending_print_width.saturating_add(usize::from(width));
                        }
                    }
                    TerminalPreParserAction::CaptureDeletedTopRows { count } => {
                        if let Some(pending) = bytes.get(flush_start..index) {
                            self.parser.process(pending);
                        }
                        pending_print_width = 0;
                        // `CSI M` only transfers rows to muxr history when deletion starts at the top
                        // row of a normal-screen top-starting region; vt100 does not put deleted
                        // lines into scrollback even when the region is full-height.
                        let (cursor_row, _) = self.parser.screen().cursor_position();
                        if cursor_row == 0 {
                            self.capture_top_rows(count);
                            flush_start = index;
                        } else if let Some(pending) = bytes.get(flush_start..index.saturating_add(1)) {
                            self.parser.process(pending);
                            flush_start = index.saturating_add(1);
                        }
                    }
                    // Codex scrolls its transcript with a top-starting partial scroll region. vt100 moves those rows
                    // out of the visible region without adding them to scrollback, so muxr captures
                    // them before feeding the final `S` byte that makes vt100 perform the scroll.
                    TerminalPreParserAction::CaptureTopRows { count } => {
                        if let Some(pending) = bytes.get(flush_start..index) {
                            self.parser.process(pending);
                        }
                        pending_print_width = 0;
                        self.capture_top_rows(count);
                        flush_start = index;
                    }
                    TerminalPreParserAction::SyncParser => {
                        if let Some(pending) = bytes.get(flush_start..index.saturating_add(1)) {
                            self.parser.process(pending);
                        }
                        pending_print_width = 0;
                        flush_start = index.saturating_add(1);
                    }
                }
            }
            if reset_here {
                if let Some(pending) = bytes.get(flush_start..index.saturating_add(1)) {
                    self.parser.process(pending);
                }
                // `vt100` resets its screen for RIS (`ESC c`) without invoking callbacks, so muxr-owned modes tracked
                // in callbacks must reset at the same byte boundary.
                self.parser.callbacks_mut().clear_tracked_application_modes();
                pending_print_width = 0;
                flush_start = index.saturating_add(1);
            }
        }
        if let Some(pending) = bytes.get(flush_start..) {
            self.parser.process(pending);
        }
    }

    fn live_row_cells(&self, row_count: usize) -> Vec<Vec<RenderCell>> {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        self::screen_row_cells(screen, rows, cols, row_count)
    }

    fn total_scrollback_len(&self) -> usize {
        self.scrollback.captured_len()
    }

    fn visible_row_cells(&self, screen_rows: u16) -> Vec<Vec<RenderCell>> {
        let height = usize::from(screen_rows);
        let offset = self.scrollback.viewport_offset();
        let mut rows = Vec::with_capacity(height);

        if offset == 0 {
            return self.live_row_cells(height);
        }

        let captured_rows = self.scrollback.captured_row_cells_for_view(offset, height);
        rows.extend(captured_rows.into_iter().take(height));
        rows.extend(self.live_row_cells(height.saturating_sub(rows.len())));
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

fn write_scrollback_dump_rows(
    rows: &[Vec<RenderCell>],
    style: ScrollbackDumpStyle,
    writer: &mut impl Write,
) -> std::io::Result<()> {
    for row in rows {
        self::write_scrollback_dump_row(row, style, writer)?;
    }
    Ok(())
}

fn write_scrollback_dump_row(
    row: &[RenderCell],
    style: ScrollbackDumpStyle,
    writer: &mut impl Write,
) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    match style {
        ScrollbackDumpStyle::PlainText => self::encode_plain_scrollback_dump_row(row, &mut bytes),
        ScrollbackDumpStyle::Ansi => self::encode_ansi_scrollback_dump_row(row, &mut bytes),
    }
    bytes.push(b'\n');
    writer.write_all(&bytes)
}

fn encode_plain_scrollback_dump_row(row: &[RenderCell], bytes: &mut Vec<u8>) {
    for cell in self::trimmed_dump_cells(row) {
        if cell.width() == RenderCellWidth::WideContinuation {
            continue;
        }
        bytes.extend_from_slice(cell.text().as_bytes());
    }
}

fn encode_ansi_scrollback_dump_row(row: &[RenderCell], bytes: &mut Vec<u8>) {
    let mut active_style = RenderStyle::default();
    for cell in self::trimmed_dump_cells(row) {
        if cell.width() == RenderCellWidth::WideContinuation {
            continue;
        }
        if cell.style() != active_style {
            self::push_sgr(cell.style(), bytes);
            active_style = cell.style();
        }
        bytes.extend_from_slice(cell.text().as_bytes());
    }
    if active_style != RenderStyle::default() {
        bytes.extend_from_slice(b"\x1b[0m");
    }
}

fn trimmed_dump_cells(row: &[RenderCell]) -> &[RenderCell] {
    let mut cells = row;
    while let Some((last, rest)) = cells.split_last() {
        if last.width() == RenderCellWidth::WideContinuation || last.text() != " " {
            break;
        }
        cells = rest;
    }
    cells
}

fn push_sgr(style: RenderStyle, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(b"\x1b[0");
    self::push_text_style_sgr(style.attrs, bytes);
    self::push_color_sgr(38, style.fg, bytes);
    self::push_color_sgr(48, style.bg, bytes);
    bytes.push(b'm');
}

fn push_text_style_sgr(attrs: RenderTextStyle, bytes: &mut Vec<u8>) {
    for (enabled, code) in [
        (attrs.bold(), "1"),
        (attrs.dim(), "2"),
        (attrs.italic(), "3"),
        (attrs.underline(), "4"),
        (attrs.inverse(), "7"),
    ] {
        if enabled {
            bytes.push(b';');
            bytes.extend_from_slice(code.as_bytes());
        }
    }
}

fn push_color_sgr(prefix: u8, color: RenderColor, bytes: &mut Vec<u8>) {
    match color {
        RenderColor::Default => {}
        RenderColor::Indexed(index) => {
            bytes.push(b';');
            bytes.extend_from_slice(prefix.to_string().as_bytes());
            bytes.extend_from_slice(b";5;");
            bytes.extend_from_slice(index.to_string().as_bytes());
        }
        RenderColor::Rgb { r, g, b } => {
            bytes.push(b';');
            bytes.extend_from_slice(prefix.to_string().as_bytes());
            bytes.extend_from_slice(b";2;");
            bytes.extend_from_slice(r.to_string().as_bytes());
            bytes.push(b';');
            bytes.extend_from_slice(g.to_string().as_bytes());
            bytes.push(b';');
            bytes.extend_from_slice(b.to_string().as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use muxr_config::MuxrConfig;
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
        let mut terminal = self::terminal_state(&terminal_size()?);

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
        let mut terminal = self::terminal_state(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(bytes).into_replies(), vec![expected.to_vec()]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_cursor_report_requested_returns_current_cursor() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(b"\x1b[2;3H").into_replies(), Vec::<Vec<u8>>::new());

        pretty_assertions::assert_eq!(terminal.process(b"\x1b[6n").into_replies(), vec![b"\x1b[2;3R".to_vec()]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_report_sequence_is_split_returns_one_reply() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(b"\x1b[").into_replies(), Vec::<Vec<u8>>::new());

        pretty_assertions::assert_eq!(terminal.process(b"6n").into_replies(), vec![b"\x1b[1;1R".to_vec()]);
        Ok(())
    }

    #[rstest]
    #[case::osc_zero(b"\x1b]0;cargo test\x07")]
    #[case::osc_two(b"\x1b]2;cargo test\x07")]
    fn test_terminal_state_title_when_window_title_is_set_returns_title(#[case] bytes: &[u8]) -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(bytes).into_replies(), Vec::<Vec<u8>>::new());

        pretty_assertions::assert_eq!(terminal.title(), Some("cargo test".to_owned()));
        Ok(())
    }

    #[test]
    fn test_terminal_state_take_title_changes_when_window_title_changes_returns_once() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

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
        let mut terminal = self::terminal_state(&terminal_size()?);

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
        let mut terminal = self::terminal_state(&terminal_size()?);

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
        let mut terminal = self::terminal_state(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(b"\x1b]2;").into_replies(), Vec::<Vec<u8>>::new());
        pretty_assertions::assert_eq!(terminal.process(b"gst\x07").into_replies(), Vec::<Vec<u8>>::new());

        pretty_assertions::assert_eq!(terminal.title(), Some("gst".to_owned()));
        Ok(())
    }

    #[test]
    fn test_terminal_state_title_when_window_title_is_empty_returns_none() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

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
        let mut terminal = self::terminal_state(&terminal_size()?);

        let outcome = terminal.process(bytes);

        assert2::assert!(!outcome.screen_dirty());
        pretty_assertions::assert_eq!(outcome.into_replies(), Vec::<Vec<u8>>::new());
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_title_sequence_is_split_keeps_screen_clean() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

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
        let mut terminal = self::terminal_state(&terminal_size()?);

        assert2::assert!(terminal.process(bytes).screen_dirty());
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_output_exceeds_viewport_shows_history() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"one\ntwo\nthree");

        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);
        assert2::assert!(rendered.contains("one"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_large_normal_output_finishes_shows_capped_history() -> rootcause::Result<()> {
        let mut scrollback = MuxrConfig::default().scrollback;
        scrollback.rows = 6;
        let mut terminal = TerminalState::with_scrollback(&TerminalSize::new(8, 4)?, scrollback);
        let mut output = String::new();
        for row in 0..20 {
            write!(output, "row-{row:02}\r\n").context("failed to format test output")?;
        }

        let _ = terminal.process(output.as_bytes());

        assert2::assert!(terminal.scroll_one_line(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("row-16"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_layout_pane_grows_before_output_captures_full_history() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);
        terminal.resize(&TerminalSize::new(8, 4)?);
        let mut output = String::new();
        for row in 0..20 {
            write!(output, "row-{row:02}\r\n").context("failed to format test output")?;
        }

        let _ = terminal.process(output.as_bytes());
        for _ in 0..20 {
            terminal.scroll_one_line(PaneScrollDirection::Up);
        }
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("row-00"));
        assert2::assert!(rendered.contains("row-03"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_replayed_history_left_alternate_screen_captures_later_shell_output()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"\x1b[?1049h");
        terminal.clear_replayed_application_state();
        let _ = terminal.process(b"one\r\ntwo\r\nthree");

        assert2::assert!(terminal.scroll_one_line(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("one"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_output_wraps_preserves_all_visual_rows() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"before\r\n");
        let _ = terminal.process(b"|abcdefghij|\r\n|klmnopqrst|\r\npsql> ");

        let mut rendered = Vec::new();
        rendered.push(self::snapshot_text(&terminal.snapshot()?));
        while terminal.scroll_one_line(PaneScrollDirection::Up) {
            rendered.push(self::snapshot_text(&terminal.snapshot()?));
        }
        let rendered = rendered.join("\n");

        assert2::assert!(rendered.contains("|abcdefg"));
        assert2::assert!(rendered.contains("hij|"));
        assert2::assert!(rendered.contains("|klmnopq"));
        assert2::assert!(rendered.contains("rst|"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_bottom_right_cell_is_filled_waits_for_next_printable_to_scroll()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(4, 2)?);

        let _ = terminal.process(b"top\r\nabc");
        let _ = terminal.process(b"d");

        assert2::assert!(!terminal.scroll_one_line(PaneScrollDirection::Up));

        let _ = terminal.process(b"e");

        assert2::assert!(terminal.scroll_one_line(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);
        assert2::assert!(rendered.contains("top"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_wide_printable_wraps_preserves_scrolled_row() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(4, 2)?);

        let _ = terminal.process(b"top\r\nabc");
        let _ = terminal.process("字".as_bytes());

        assert2::assert!(terminal.scroll_one_line(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);
        assert2::assert!(rendered.contains("top"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_alternate_screen_sets_partial_region_preserves_normal_scrollback()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 3)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[2;3r\x1b[?1049l");
        let _ = terminal.process(b"one\r\ntwo\r\nthree\r\nfour");

        assert2::assert!(terminal.scroll_one_line(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);
        assert2::assert!(rendered.contains("one"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scrollback_dump_when_partial_history_precedes_normal_output_keeps_chronology()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(16, 3)?);

        let _ = terminal.process(b"partial-old\r\npartial-live");
        let _ = terminal.process(b"\x1b[1;2r\x1b[2;1H\x1b[S\x1b[r");
        let _ = terminal.process(b"\x1b[3;1Hnormal-new-1\r\nnormal-new-2\r\nnormal-new-3\r\n");

        let dump = String::from_utf8(self::test_scrollback_dump(&terminal, ScrollbackDumpStyle::PlainText)?)?;

        assert2::assert!(
            let Some(partial_index) = dump.find("partial-old")
                && let Some(normal_index) = dump.find("normal-new-1")
                && partial_index < normal_index
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scrollback_dump_when_output_exceeds_viewport_returns_history_and_live_rows()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"one\r\ntwo\r\nthree");

        pretty_assertions::assert_eq!(
            String::from_utf8(self::test_scrollback_dump(&terminal, ScrollbackDumpStyle::PlainText)?)?,
            "one\ntwo\nthree\n",
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scrollback_dump_when_viewport_is_scrolled_preserves_viewport() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);
        let _ = terminal.process(b"one\r\ntwo\r\nthree");
        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let before = terminal.snapshot()?;

        let _dump = self::test_scrollback_dump(&terminal, ScrollbackDumpStyle::PlainText)?;
        let after = terminal.snapshot()?;

        pretty_assertions::assert_eq!(after, before);
        Ok(())
    }

    #[test]
    fn test_terminal_state_scrollback_dump_when_top_partial_scroll_region_moves_rows_includes_captured_rows()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[2S\x1b[r");

        pretty_assertions::assert_eq!(
            String::from_utf8(self::test_scrollback_dump(&terminal, ScrollbackDumpStyle::PlainText)?)?,
            "one\ntwo\nthree\n\n\nprompt\n",
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scrollback_dump_when_ansi_style_requested_preserves_rendered_style() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"\x1b[31mred\x1b[0m");

        pretty_assertions::assert_eq!(
            String::from_utf8(self::test_scrollback_dump(&terminal, ScrollbackDumpStyle::Ansi)?)?,
            "\x1b[0;38;5;1mred\x1b[0m\n\n",
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_to_bottom_when_scrolled_shows_live_viewport() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

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
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[2S\x1b[r");

        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("one"));
        assert2::assert!(rendered.contains("two"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_when_partial_rows_exceed_configured_limit_keeps_recent_rows() -> rootcause::Result<()> {
        let mut scrollback = MuxrConfig::default().scrollback;
        scrollback.rows = 2;
        let mut terminal = TerminalState::with_scrollback(&TerminalSize::new(8, 4)?, scrollback);

        for row in 0..4 {
            let _ = terminal.process(format!("\x1b[1;1Hrow-{row}\x1b[2;1Hstill\x1b[3;1Hprompt").as_bytes());
            let _ = terminal.process(b"\x1b[1;3r\x1b[1S\x1b[r");
        }

        pretty_assertions::assert_eq!(terminal.scrollback.captured_len(), 2);
        let retained_text = terminal
            .scrollback
            .captured_oldest_row_cells_iter(2)
            .map(|row| row.iter().map(RenderCell::text).collect::<String>())
            .collect::<Vec<_>>();
        assert2::assert!(
            retained_text
                .iter()
                .all(|row| !row.starts_with("row-0") && !row.starts_with("row-1"))
        );
        assert2::assert!(retained_text.first().is_some_and(|row| row.starts_with("row-2")));
        assert2::assert!(retained_text.get(1).is_some_and(|row| row.starts_with("row-3")));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_partial_scroll_sequence_is_split_preserves_history() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

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
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

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
    fn test_terminal_state_scroll_when_alternate_screen_linefeed_moves_rows_does_not_capture_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[3;1H\n\x1b[r");

        assert2::assert!(!terminal.scroll(PaneScrollDirection::Up));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_top_partial_scroll_region_delete_lines_moves_rows_preserves_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[1;1H\x1b[2M\x1b[r");

        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("one"));
        assert2::assert!(rendered.contains("two"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_full_scroll_region_delete_lines_moves_rows_preserves_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;4r\x1b[1;1H\x1b[2M\x1b[r");

        assert2::assert!(terminal.scroll(PaneScrollDirection::Up));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert2::assert!(rendered.contains("one"));
        assert2::assert!(rendered.contains("two"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_mid_partial_scroll_region_delete_lines_moves_rows_does_not_capture_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[2;1H\x1b[2M\x1b[r");

        assert2::assert!(!terminal.scroll(PaneScrollDirection::Up));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_alternate_screen_delete_lines_moves_rows_does_not_capture_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;4r\x1b[1;1H\x1b[2M\x1b[r");

        assert2::assert!(!terminal.scroll(PaneScrollDirection::Up));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_alternate_screen_partial_scroll_region_moves_rows_does_not_capture_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[2S\x1b[r");

        assert2::assert!(!terminal.scroll(PaneScrollDirection::Up));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_alternate_screen_full_scroll_region_moves_rows_does_not_capture_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;4r\x1b[2S\x1b[r");

        assert2::assert!(!terminal.scroll(PaneScrollDirection::Up));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_normal_screen_full_scroll_region_moves_rows_preserves_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

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
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

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
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(b"\x1b[?2004h");

        assert2::assert!(terminal.bracketed_paste_enabled());
        Ok(())
    }

    #[test]
    fn test_terminal_state_mouse_protocol_when_sgr_button_motion_is_enabled_returns_protocol() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&terminal_size()?);

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
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(bytes);

        pretty_assertions::assert_eq!(
            terminal.application_mode(),
            TerminalApplicationMode {
                screen_mode: TerminalScreenMode::from_alternate_screen(expected),
                cursor_key_mode: TerminalCursorKeyMode::Normal,
                keyboard_protocol: TerminalKeyboardProtocol::Legacy,
                focus_reporting: TerminalFocusReporting::Disabled,
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
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(bytes);

        pretty_assertions::assert_eq!(
            terminal.application_mode(),
            TerminalApplicationMode {
                screen_mode: TerminalScreenMode::Normal,
                cursor_key_mode: TerminalCursorKeyMode::from_application_cursor(expected),
                keyboard_protocol: TerminalKeyboardProtocol::Legacy,
                focus_reporting: TerminalFocusReporting::Disabled,
                mouse_protocol: None,
            },
        );
        Ok(())
    }

    #[rstest]
    #[case::enabled(b"\x1b[?1004h", TerminalFocusReporting::Enabled)]
    #[case::disabled(b"\x1b[?1004h\x1b[?1004l", TerminalFocusReporting::Disabled)]
    #[case::disabled_by_terminal_reset(b"\x1b[?1004h\x1bc", TerminalFocusReporting::Disabled)]
    #[case::enabled_after_terminal_reset(b"\x1b[?1004h\x1bc\x1b[?1004h", TerminalFocusReporting::Enabled)]
    #[case::enabled_with_other_private_modes(b"\x1b[?1;1004h", TerminalFocusReporting::Enabled)]
    fn test_terminal_state_application_mode_when_focus_reporting_changes_returns_state(
        #[case] bytes: &[u8],
        #[case] expected: TerminalFocusReporting,
    ) -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(bytes);

        pretty_assertions::assert_eq!(
            terminal.application_mode(),
            TerminalApplicationMode {
                screen_mode: TerminalScreenMode::Normal,
                cursor_key_mode: TerminalCursorKeyMode::from_application_cursor(bytes == b"\x1b[?1;1004h"),
                keyboard_protocol: TerminalKeyboardProtocol::Legacy,
                focus_reporting: expected,
                mouse_protocol: None,
            },
        );
        Ok(())
    }

    #[rstest]
    #[case::enabled_by_push(b"\x1b[>1u", TerminalKeyboardProtocol::KittyLevelOne)]
    #[case::disabled_by_push_zero(b"\x1b[>1u\x1b[>0u", TerminalKeyboardProtocol::Legacy)]
    #[case::disabled_by_pop(b"\x1b[>1u\x1b[<u", TerminalKeyboardProtocol::Legacy)]
    #[case::enabled_by_set(b"\x1b[=1u", TerminalKeyboardProtocol::KittyLevelOne)]
    #[case::disabled_by_set_zero(b"\x1b[=1u\x1b[=0u", TerminalKeyboardProtocol::Legacy)]
    #[case::disabled_by_set_replace_without_disambiguate_bit(b"\x1b[=2u", TerminalKeyboardProtocol::Legacy)]
    #[case::disabled_by_set_difference(b"\x1b[>1u\x1b[=1;3u", TerminalKeyboardProtocol::Legacy)]
    #[case::disabled_by_terminal_reset(b"\x1b[>1u\x1bc", TerminalKeyboardProtocol::Legacy)]
    #[case::disabled_by_terminal_reset_clears_keyboard_protocol(
        b"\x1b[>1u\x1bc\x1b[<u",
        TerminalKeyboardProtocol::Legacy
    )]
    fn test_terminal_state_application_mode_when_keyboard_protocol_changes_returns_state(
        #[case] bytes: &[u8],
        #[case] expected: TerminalKeyboardProtocol,
    ) -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(bytes);

        pretty_assertions::assert_eq!(terminal.application_mode().keyboard_protocol, expected,);
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_keyboard_protocol_is_queried_returns_status() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        pretty_assertions::assert_eq!(
            terminal.process(b"\x1b[?u").into_replies(),
            vec![KITTY_KEYBOARD_PROTOCOL_DISABLED_REPLY.to_vec()],
        );
        let _ = terminal.process(b"\x1b[>1u");
        pretty_assertions::assert_eq!(
            terminal.process(b"\x1b[?u").into_replies(),
            vec![KITTY_KEYBOARD_PROTOCOL_ENABLED_REPLY.to_vec()],
        );
        let _ = terminal.process(b"\x1b[<u");
        pretty_assertions::assert_eq!(
            terminal.process(b"\x1b[?u").into_replies(),
            vec![KITTY_KEYBOARD_PROTOCOL_DISABLED_REPLY.to_vec()],
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_clear_replayed_application_state_clears_keyboard_protocol() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(b"\x1b[>1u");
        terminal.clear_replayed_application_state();

        pretty_assertions::assert_eq!(
            terminal.application_mode().keyboard_protocol,
            TerminalKeyboardProtocol::Legacy,
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_application_mode_when_terminal_reset_sequence_is_split_clears_focus_reporting()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(b"\x1b[?1004h\x1b");
        let _ = terminal.process(b"c");

        pretty_assertions::assert_eq!(
            terminal.application_mode().focus_reporting,
            TerminalFocusReporting::Disabled,
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_application_mode_when_mouse_protocol_is_enabled_returns_protocol() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(b"\x1b[?1002h\x1b[?1006h");

        pretty_assertions::assert_eq!(
            terminal.application_mode(),
            TerminalApplicationMode {
                screen_mode: TerminalScreenMode::Normal,
                cursor_key_mode: TerminalCursorKeyMode::Normal,
                keyboard_protocol: TerminalKeyboardProtocol::Legacy,
                focus_reporting: TerminalFocusReporting::Disabled,
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
        let mut terminal = self::terminal_state(&terminal_size()?);

        pretty_assertions::assert_eq!(terminal.process(bytes).into_replies(), Vec::<Vec<u8>>::new());
        Ok(())
    }

    fn terminal_size() -> rootcause::Result<TerminalSize> {
        TerminalSize::new(8, 4)
    }

    fn terminal_state(size: &TerminalSize) -> TerminalState {
        TerminalState::with_scrollback(size, MuxrConfig::default().scrollback)
    }

    fn test_scrollback_dump(terminal: &TerminalState, style: ScrollbackDumpStyle) -> rootcause::Result<Vec<u8>> {
        Ok(terminal.scrollback_dump(style)?)
    }

    fn snapshot_text(snapshot: &TerminalSnapshot) -> String {
        snapshot
            .rows()
            .iter()
            .flat_map(|row| row.cells().iter().map(RenderCell::text))
            .collect()
    }
}
