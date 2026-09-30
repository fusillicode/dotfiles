use std::ops::Range;

use muxr_config::ScrollbackConfig;
use muxr_config::ScrollbackDumpStyle;
use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::PaneMouseMode;
use muxr_core::PaneScrollDirection;
use muxr_core::RenderCursor;
use muxr_core::RenderRowSpan;
use muxr_core::RowWrap;
use muxr_core::TerminalSize;
use rio_vt::crosswords::Mode;
use rio_vt::crosswords::grid::Dimensions;
use rio_vt::crosswords::grid::Scroll;
use rio_vt::event::TerminalDamage;
use rootcause::prelude::ResultExt;
use smallvec::SmallVec;

use self::control::ControlParser;
use self::control::CursorControl;
use self::rio::RioInputFilter;
use self::rio::RioTerminal;

mod control;
mod render;
mod rio;

#[cfg(test)]
mod tests;

const SCROLL_LINES_PER_WHEEL_EVENT: usize = 5;
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const KITTY_KEYBOARD_PROTOCOL_DISAMBIGUATE_ESC_CODES_MODE: u16 = 1;

/// Terminal replies generated while parsing PTY output.
///
/// Reply batches are normally empty or a single terminal-generated response, such as DSR/CPR or keyboard protocol
/// status, so the outer buffer stays inline while callers use [`AsRef`] at writer boundaries that still accept
/// `&[Vec<u8>]`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalReplies(SmallVec<[Vec<u8>; 2]>);

impl TerminalReplies {
    fn push(&mut self, reply: Vec<u8>) {
        self.0.push(reply);
    }
}

impl AsRef<[Vec<u8>]> for TerminalReplies {
    fn as_ref(&self) -> &[Vec<u8>] {
        self.0.as_slice()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalSnapshotScope {
    ChangedRows,
    Full,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalSnapshot {
    cursor: RenderCursor,
    row_wraps: Vec<RowWrap>,
    rows: Vec<RenderRowSpan>,
    scope: TerminalSnapshotScope,
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
    pub fn row_wraps(&self) -> &[RowWrap] {
        &self.row_wraps
    }

    #[must_use]
    pub const fn size(&self) -> &TerminalSize {
        &self.size
    }

    pub(crate) fn apply_update(&mut self, update: Self) -> rootcause::Result<Vec<u16>> {
        if self.size != update.size {
            return Err(rootcause::report!("muxr terminal snapshot update changed size"));
        }
        self.cursor = update.cursor;
        self.row_wraps = update.row_wraps;
        let changed_rows = update.rows.iter().map(RenderRowSpan::row).collect();
        if matches!(update.scope, TerminalSnapshotScope::Full) {
            self.rows = update.rows;
            return Ok(changed_rows);
        }
        for row in update.rows {
            let target = self
                .rows
                .get_mut(usize::from(row.row()))
                .ok_or_else(|| rootcause::report!("muxr terminal snapshot update row is outside its cache"))?;
            *target = row;
        }
        Ok(changed_rows)
    }
}

/// Text candidates starting at each physical row, including any soft-wrapped successors.
#[derive(Default)]
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct TerminalTextTail {
    text: String,
    lines: Vec<Range<usize>>,
}

impl TerminalTextTail {
    pub(crate) fn candidate_lines(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.lines
            .iter()
            .filter_map(|range| self.text.get(range.clone()))
            .map(str::trim_end)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CursorShapeSource {
    Default,
    Explicit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderedViewport {
    Live,
    Scrolled { top_row: u64 },
}

pub struct TerminalState {
    input_filter: RioInputFilter,
    control_parser: ControlParser,
    cursor_shape_source: CursorShapeSource,
    rendered_viewport: Option<RenderedViewport>,
    rio: RioTerminal,
    title: Option<String>,
    title_changes: Vec<Option<String>>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalScreenDmg {
    #[default]
    Clean,
    Dirty,
}

/// Result of feeding PTY bytes into the terminal parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalProcessOutcome {
    Clean { replies: TerminalReplies },
    MetadataDirty { replies: TerminalReplies },
    ScreenDirty { replies: TerminalReplies },
}

impl TerminalProcessOutcome {
    #[must_use]
    pub fn into_replies(self) -> TerminalReplies {
        match self {
            Self::Clean { replies } | Self::MetadataDirty { replies } | Self::ScreenDirty { replies } => replies,
        }
    }

    #[must_use]
    pub const fn screen_dmg(&self) -> TerminalScreenDmg {
        match self {
            Self::Clean { .. } => TerminalScreenDmg::Clean,
            Self::MetadataDirty { .. } | Self::ScreenDirty { .. } => TerminalScreenDmg::Dirty,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalScrollMove {
    #[default]
    Unchanged,
    Moved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalPasteMode {
    Plain,
    Bracketed,
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
    pub const fn event_report(self, event: ClientMouseEvent) -> TerminalMouseEventReport {
        let is_motion = event.button & 32 != 0;
        let is_release = matches!(event.phase, ClientMouseEventPhase::Release);
        let report = match self.mode {
            TerminalMouseProtocolMode::Press => !is_release && !is_motion,
            TerminalMouseProtocolMode::PressRelease => !is_motion,
            // `?1002` button-motion panes must not receive `?1003` hover packets from the outer terminal.
            TerminalMouseProtocolMode::ButtonMotion => !(event.button & 32 != 0 && event.button & 0b11 == 0b11),
            TerminalMouseProtocolMode::AnyMotion => true,
        };
        if report {
            TerminalMouseEventReport::Report
        } else {
            TerminalMouseEventReport::Drop
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalMouseEventReport {
    Drop,
    Report,
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

impl From<u16> for TerminalKeyboardProtocol {
    fn from(mode: u16) -> Self {
        if mode & KITTY_KEYBOARD_PROTOCOL_DISAMBIGUATE_ESC_CODES_MODE == 0 {
            Self::Legacy
        } else {
            Self::KittyLevelOne
        }
    }
}

/// Terminal screen buffer selected by the pane application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalScreenMode {
    Alternate,
    Normal,
}

/// Cursor-key escape sequence mode selected by the pane application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalCursorKeyMode {
    Application,
    Normal,
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

impl TerminalState {
    pub fn with_scrollback(size: &TerminalSize, scrollback: ScrollbackConfig) -> Self {
        Self {
            input_filter: RioInputFilter::default(),
            control_parser: ControlParser::default(),
            cursor_shape_source: CursorShapeSource::Default,
            rendered_viewport: None,
            rio: RioTerminal::new(usize::from(size.cols()), usize::from(size.rows()), scrollback.rows),
            title: None,
            title_changes: Vec::new(),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) -> TerminalProcessOutcome {
        if bytes.is_empty() {
            return TerminalProcessOutcome::Clean {
                replies: TerminalReplies::default(),
            };
        }

        let cursor_before = self.rio.terminal().cursor_shape;
        let blinking_before = self.rio.terminal().blinking_cursor;
        let cursor_visibility_before = self.rio.terminal().cursor().is_visible();
        let mouse_protocol_before = self.mouse_protocol();

        let filtered_input = self.input_filter.process(bytes);
        let bytes = filtered_input.as_ref();
        let terminal_control = self.control_parser.process(bytes);
        let events = self
            .rio
            .advance_with_alternate_screen_controls(bytes, &terminal_control.alternate_screen);

        let terminal = self.rio.terminal();
        let cursor_values_changed =
            terminal.cursor_shape != cursor_before || terminal.blinking_cursor != blinking_before;

        self.cursor_shape_source = match terminal_control.cursor {
            CursorControl::DefaultShape | CursorControl::Reset => CursorShapeSource::Default,
            CursorControl::ExplicitShape => CursorShapeSource::Explicit,
            CursorControl::Unchanged if cursor_values_changed => {
                if terminal.cursor_shape == terminal.default_cursor_shape && !terminal.blinking_cursor {
                    CursorShapeSource::Default
                } else {
                    CursorShapeSource::Explicit
                }
            }
            CursorControl::Unchanged => self.cursor_shape_source,
        };

        let mut replies = TerminalReplies::default();
        for reply in events.replies {
            replies.push(reply);
        }

        for title in events.titles {
            if self.title != title {
                self.title.clone_from(&title);
                self.title_changes.push(title);
            }
        }

        let cursor_changed = terminal_control.cursor != CursorControl::Unchanged
            || cursor_values_changed
            || events.cursor_change == self::rio::CursorChange::Changed;

        let metadata_changed = cursor_visibility_before != terminal.cursor().is_visible()
            || mouse_protocol_before != self.mouse_protocol();

        if self.rio.terminal().peek_damage_event().is_some() {
            TerminalProcessOutcome::ScreenDirty { replies }
        } else if cursor_changed || metadata_changed {
            TerminalProcessOutcome::MetadataDirty { replies }
        } else {
            TerminalProcessOutcome::Clean { replies }
        }
    }

    pub fn resize(&mut self, size: &TerminalSize) {
        self.rio.resize(usize::from(size.cols()), usize::from(size.rows()));
    }

    pub fn title(&self) -> Option<String> {
        self.title.clone()
    }

    pub fn take_title_changes(&mut self) -> Vec<Option<String>> {
        std::mem::take(&mut self.title_changes)
    }

    pub fn scroll(&mut self, direction: PaneScrollDirection) -> TerminalScrollMove {
        self.scroll_lines(direction, SCROLL_LINES_PER_WHEEL_EVENT)
    }

    pub fn scroll_one_line(&mut self, direction: PaneScrollDirection) -> TerminalScrollMove {
        self.scroll_lines(direction, 1)
    }

    fn scroll_lines(&mut self, direction: PaneScrollDirection, lines: usize) -> TerminalScrollMove {
        let before = self.rio.terminal().display_offset();
        let lines = i32::try_from(lines).unwrap_or(i32::MAX);
        let delta = match direction {
            PaneScrollDirection::Down => lines.saturating_neg(),
            PaneScrollDirection::Up => lines,
        };
        self.rio.terminal_mut().scroll_display(Scroll::Delta(delta));
        Self::scroll_move(before, self.rio.terminal().display_offset())
    }

    pub fn scroll_to_bottom(&mut self) -> TerminalScrollMove {
        let before = self.rio.terminal().display_offset();
        self.rio.terminal_mut().scroll_display(Scroll::Bottom);
        Self::scroll_move(before, self.rio.terminal().display_offset())
    }

    pub fn visible_top_row(&self) -> rootcause::Result<u64> {
        let grid = &self.rio.terminal().grid;
        let retained_top = grid.history_size().saturating_sub(grid.display_offset());
        let retained_top = u64::try_from(retained_top).context("muxr pane visible top row overflowed")?;
        Ok(grid.lines_evicted().saturating_add(retained_top))
    }

    fn rendered_viewport(&self) -> rootcause::Result<RenderedViewport> {
        if self.rio.terminal().display_offset() == 0 {
            Ok(RenderedViewport::Live)
        } else {
            Ok(RenderedViewport::Scrolled {
                top_row: self.visible_top_row()?,
            })
        }
    }

    pub fn visible_row_wraps(&self) -> Vec<RowWrap> {
        let terminal = self.rio.terminal();
        (0..terminal.screen_lines())
            .map(|row| render::row_wrap(&terminal.grid[render::visible_line(terminal, row)]))
            .collect()
    }

    pub fn paste_mode(&self) -> TerminalPasteMode {
        if self.rio.terminal().mode().contains(Mode::BRACKETED_PASTE) {
            TerminalPasteMode::Bracketed
        } else {
            TerminalPasteMode::Plain
        }
    }

    pub fn application_mode(&self) -> TerminalApplicationMode {
        let terminal = self.rio.terminal();
        let mode = terminal.mode();

        TerminalApplicationMode {
            screen_mode: if mode.contains(Mode::ALT_SCREEN) {
                TerminalScreenMode::Alternate
            } else {
                TerminalScreenMode::Normal
            },
            cursor_key_mode: if mode.contains(Mode::APP_CURSOR) {
                TerminalCursorKeyMode::Application
            } else {
                TerminalCursorKeyMode::Normal
            },
            keyboard_protocol: TerminalKeyboardProtocol::from(u16::from(terminal.keyboard_mode().bits())),
            focus_reporting: if mode.contains(Mode::FOCUS_IN_OUT) {
                TerminalFocusReporting::Enabled
            } else {
                TerminalFocusReporting::Disabled
            },
            mouse_protocol: self.mouse_protocol(),
        }
    }

    pub fn mouse_protocol(&self) -> Option<TerminalMouseProtocol> {
        let terminal_mode = self.rio.terminal().mode();
        let mode = if terminal_mode.contains(Mode::MOUSE_MOTION) {
            TerminalMouseProtocolMode::AnyMotion
        } else if terminal_mode.contains(Mode::MOUSE_DRAG) {
            TerminalMouseProtocolMode::ButtonMotion
        } else if terminal_mode.contains(Mode::MOUSE_REPORT_CLICK) {
            TerminalMouseProtocolMode::PressRelease
        } else if terminal_mode.contains(Mode::MOUSE_REPORT_X10) {
            TerminalMouseProtocolMode::Press
        } else {
            return None;
        };

        let encoding = if terminal_mode.contains(Mode::SGR_MOUSE) {
            TerminalMouseProtocolEncoding::Sgr
        } else if terminal_mode.contains(Mode::UTF8_MOUSE) {
            TerminalMouseProtocolEncoding::Utf8
        } else {
            TerminalMouseProtocolEncoding::Default
        };

        Some(TerminalMouseProtocol { encoding, mode })
    }

    pub fn render_snapshot(&mut self, requested_scope: TerminalSnapshotScope) -> rootcause::Result<TerminalSnapshot> {
        let rendered_viewport = self.rendered_viewport()?;
        let viewport_changed = self
            .rendered_viewport
            .is_none_or(|previous_viewport| previous_viewport != rendered_viewport);

        let scope = if matches!(requested_scope, TerminalSnapshotScope::Full)
            || matches!(self.rio.terminal().peek_damage_event(), Some(TerminalDamage::Full))
            || viewport_changed
        {
            TerminalSnapshotScope::Full
        } else {
            TerminalSnapshotScope::ChangedRows
        };

        let snapshot = self.snapshot_rows(scope)?;
        let terminal = self.rio.terminal_mut();
        for row in snapshot.rows() {
            let line = render::visible_line(terminal, usize::from(row.row()));
            terminal.grid[line].dirty = false;
        }
        terminal.reset_damage();
        self.rendered_viewport = Some(rendered_viewport);

        Ok(snapshot)
    }

    fn snapshot_rows(&self, scope: TerminalSnapshotScope) -> rootcause::Result<TerminalSnapshot> {
        render::snapshot_rows(self.rio.terminal(), scope, self.cursor_shape_source)
    }

    pub fn scrollback_dump(&mut self, style: ScrollbackDumpStyle) -> Vec<u8> {
        let terminal = self.rio.terminal();
        let history = i32::try_from(terminal.grid.history_size()).unwrap_or(i32::MAX);
        let screen_lines = i32::try_from(terminal.screen_lines()).unwrap_or(i32::MAX);
        if !terminal.mode().contains(Mode::ALT_SCREEN) {
            return render::scrollback_grid_dump(terminal, history.saturating_neg()..screen_lines, style);
        }

        let alternate_dump = render::scrollback_grid_dump(terminal, 0..screen_lines, style);
        let mut dump = self.rio.with_primary_screen(|primary| {
            let history = i32::try_from(primary.grid.history_size()).unwrap_or(i32::MAX);
            render::scrollback_grid_dump(primary, history.saturating_neg()..0, style)
        });
        dump.extend_from_slice(&alternate_dump);
        dump
    }

    /// Read the bottom live rows without moving the viewport or consuming render damage.
    pub(crate) fn live_tail_text(&self, row_limit: usize) -> TerminalTextTail {
        render::live_tail_text(self.rio.terminal(), row_limit)
    }

    const fn scroll_move(before: usize, after: usize) -> TerminalScrollMove {
        if before == after {
            TerminalScrollMove::Unchanged
        } else {
            TerminalScrollMove::Moved
        }
    }
}

pub fn paste_input_bytes(bytes: &[u8], paste_mode: TerminalPasteMode) -> Vec<u8> {
    match paste_mode {
        TerminalPasteMode::Plain => bytes.to_vec(),
        TerminalPasteMode::Bracketed => {
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
    }
}
