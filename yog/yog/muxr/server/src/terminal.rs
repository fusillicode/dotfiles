use std::collections::HashMap;

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
use muxr_core::RenderCursorShape;
use muxr_core::RenderHyperlink;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderTextStyle;
use muxr_core::RowWrap;
use muxr_core::TerminalSize;
use rio_vt::ansi::CursorShape as RioCursorShape;
use rio_vt::config::colors::AnsiColor;
use rio_vt::config::colors::NamedColor;
use rio_vt::crosswords::Crosswords;
use rio_vt::crosswords::Mode;
use rio_vt::crosswords::grid::Dimensions;
use rio_vt::crosswords::grid::Grid;
use rio_vt::crosswords::grid::Scroll;
use rio_vt::crosswords::grid::row::Row;
use rio_vt::crosswords::pos::Column;
use rio_vt::crosswords::pos::Line;
use rio_vt::crosswords::pos::Pos;
use rio_vt::crosswords::square::CellFlags;
use rio_vt::crosswords::square::ContentTag;
use rio_vt::crosswords::square::ExtrasId;
use rio_vt::crosswords::square::Square;
use rio_vt::crosswords::square::Wide;
use rio_vt::crosswords::style::StyleFlags;
use rio_vt::event::EventListener;
use rio_vt::event::TerminalDamage;
use rootcause::prelude::ResultExt;
use smallvec::SmallVec;

use self::rio::RioInputFilter;
use self::rio::RioTerminal;

mod rio;

const SCROLL_LINES_PER_WHEEL_EVENT: usize = 5;
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
#[cfg(test)]
const KITTY_KEYBOARD_PROTOCOL_DISABLED_REPLY: &[u8] = b"\x1b[?0u";
#[cfg(test)]
const KITTY_KEYBOARD_PROTOCOL_ENABLED_REPLY: &[u8] = b"\x1b[?1u";
const KITTY_KEYBOARD_PROTOCOL_DISAMBIGUATE_ESC_CODES_MODE: u16 = 1;
const OSC_CURSOR_SHAPE_PREFIX: &[u8] = b"CursorShape=";
const RENDER_CELL_TEXT_INLINE_BYTES: usize = 24;

/// Terminal replies generated while parsing PTY output.
///
/// Reply batches are normally empty or a single terminal-generated response, such as DSR/CPR or keyboard protocol
/// status, so the outer buffer stays inline while callers use [`Self::as_slice`] or [`AsRef`] at writer boundaries that
/// still accept `&[Vec<u8>]`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalReplies(SmallVec<[Vec<u8>; 2]>);

impl TerminalReplies {
    #[must_use]
    pub fn as_slice(&self) -> &[Vec<u8>] {
        self.0.as_slice()
    }

    fn push(&mut self, reply: Vec<u8>) {
        self.0.push(reply);
    }
}

impl AsRef<[Vec<u8>]> for TerminalReplies {
    fn as_ref(&self) -> &[Vec<u8>] {
        self.as_slice()
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CursorShapeSource {
    Default,
    Explicit,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CursorControl {
    #[default]
    Unchanged,
    DefaultShape,
    ExplicitShape,
    Reset,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CursorControlState {
    #[default]
    Ground,
    Escape,
    CsiParameter(CursorParameter),
    CsiPrivateParameter {
        alternate_screen: AlternateScreenParameter,
        parameter: CursorParameter,
    },
    CsiSpace(CursorParameter),
    CsiInvalid,
    OscCursorShape(OscCursorShapeState),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OscCursorShapeState {
    CommandStart,
    CommandFive,
    CommandFifty,
    Prefix(usize),
    Value,
    Valid,
    Invalid,
}

impl OscCursorShapeState {
    fn observe(self, byte: u8) -> Self {
        match self {
            Self::CommandStart if byte == b'5' => Self::CommandFive,
            Self::CommandFive if byte == b'0' => Self::CommandFifty,
            Self::CommandFifty if byte == b';' => Self::Prefix(0),
            Self::Prefix(index) if OSC_CURSOR_SHAPE_PREFIX.get(index) == Some(&byte) => {
                let next = index.saturating_add(1);
                if next == OSC_CURSOR_SHAPE_PREFIX.len() {
                    Self::Value
                } else {
                    Self::Prefix(next)
                }
            }
            Self::Value if matches!(byte, b'0'..=b'2') => Self::Valid,
            Self::Valid => Self::Valid,
            Self::CommandStart
            | Self::CommandFive
            | Self::CommandFifty
            | Self::Prefix(_)
            | Self::Value
            | Self::Invalid => Self::Invalid,
        }
    }

    const fn cursor_control(self) -> CursorControl {
        if matches!(self, Self::Valid) {
            CursorControl::ExplicitShape
        } else {
            CursorControl::Unchanged
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CursorParameter {
    Empty,
    Value(u16),
    Invalid,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TerminalControlAction {
    AlternateScreen(AlternateScreenControl),
    Cursor(CursorControl),
    Reset,
    #[default]
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AlternateScreenControl {
    EnterLegacy,
    ExitLegacy,
    InvalidatePreserved,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AlternateScreenParameter {
    #[default]
    Absent,
    Legacy,
    Native,
}

impl AlternateScreenParameter {
    const fn record(self, parameter: CursorParameter) -> Self {
        match (self, parameter) {
            (_, CursorParameter::Value(1049)) => Self::Native,
            (Self::Absent, CursorParameter::Value(47)) => Self::Legacy,
            (current, _) => current,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderedViewport {
    Live,
    Scrolled { top_row: u64 },
}

#[derive(Default)]
struct TerminalControlOutcome {
    alternate_screen: SmallVec<[(usize, AlternateScreenControl); 2]>,
    cursor: CursorControl,
}

#[derive(Default)]
struct TerminalControlTracker {
    state: CursorControlState,
}

impl TerminalControlTracker {
    fn process(&mut self, bytes: &[u8]) -> TerminalControlOutcome {
        let mut outcome = TerminalControlOutcome::default();
        for (index, byte) in bytes.iter().enumerate() {
            match self.observe_byte(*byte) {
                TerminalControlAction::AlternateScreen(control) => {
                    outcome.alternate_screen.push((index.saturating_add(1), control));
                }
                TerminalControlAction::Cursor(cursor) => outcome.cursor = cursor,
                TerminalControlAction::Reset => {
                    outcome.cursor = CursorControl::Reset;
                    outcome
                        .alternate_screen
                        .push((index.saturating_add(1), AlternateScreenControl::InvalidatePreserved));
                }
                TerminalControlAction::Unchanged => {}
            }
        }
        outcome
    }

    fn observe_byte(&mut self, byte: u8) -> TerminalControlAction {
        match self.state {
            CursorControlState::Ground => self.observe_ground(byte),
            CursorControlState::Escape => self.observe_escape(byte),
            CursorControlState::CsiParameter(parameter) => self.observe_csi_parameter(byte, parameter),
            CursorControlState::CsiPrivateParameter {
                alternate_screen,
                parameter,
            } => self.observe_csi_private_parameter(byte, parameter, alternate_screen),
            CursorControlState::CsiSpace(parameter) => self.observe_csi_space(byte, parameter),
            CursorControlState::CsiInvalid => self.observe_csi_invalid(byte),
            CursorControlState::OscCursorShape(state) => self.observe_osc_cursor_shape(byte, state),
        }
    }

    const fn observe_ground(&mut self, byte: u8) -> TerminalControlAction {
        self.state = match byte {
            b'\x1b' => CursorControlState::Escape,
            _ => CursorControlState::Ground,
        };
        TerminalControlAction::Unchanged
    }

    const fn observe_escape(&mut self, byte: u8) -> TerminalControlAction {
        match byte {
            b'\x1b' => self.state = CursorControlState::Escape,
            b'[' => self.state = CursorControlState::CsiParameter(CursorParameter::Empty),
            b']' => self.state = CursorControlState::OscCursorShape(OscCursorShapeState::CommandStart),
            b'c' => {
                self.state = CursorControlState::Ground;
                return TerminalControlAction::Reset;
            }
            _ => self.state = CursorControlState::Ground,
        }
        TerminalControlAction::Unchanged
    }

    fn observe_csi_parameter(&mut self, byte: u8, parameter: CursorParameter) -> TerminalControlAction {
        self.state = match byte {
            b'0'..=b'9' => CursorControlState::CsiParameter(Self::append_cursor_parameter(parameter, byte)),
            b'?' if parameter == CursorParameter::Empty => CursorControlState::CsiPrivateParameter {
                alternate_screen: AlternateScreenParameter::Absent,
                parameter: CursorParameter::Empty,
            },
            b' ' => CursorControlState::CsiSpace(parameter),
            0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f => CursorControlState::CsiParameter(parameter),
            b'\x18' | b'\x1a' | 0x40..=0x7e => CursorControlState::Ground,
            b'\x1b' => CursorControlState::Escape,
            _ => CursorControlState::CsiInvalid,
        };
        TerminalControlAction::Unchanged
    }

    fn observe_csi_private_parameter(
        &mut self,
        byte: u8,
        parameter: CursorParameter,
        alternate_screen: AlternateScreenParameter,
    ) -> TerminalControlAction {
        match byte {
            b'0'..=b'9' => {
                self.state = CursorControlState::CsiPrivateParameter {
                    alternate_screen,
                    parameter: Self::append_cursor_parameter(parameter, byte),
                };
                TerminalControlAction::Unchanged
            }
            b';' => {
                self.state = CursorControlState::CsiPrivateParameter {
                    alternate_screen: alternate_screen.record(parameter),
                    parameter: CursorParameter::Empty,
                };
                TerminalControlAction::Unchanged
            }
            0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f => {
                self.state = CursorControlState::CsiPrivateParameter {
                    alternate_screen,
                    parameter,
                };
                TerminalControlAction::Unchanged
            }
            b'h' | b'l' => {
                self.state = CursorControlState::Ground;
                match (alternate_screen.record(parameter), byte) {
                    (AlternateScreenParameter::Legacy, b'h') => {
                        TerminalControlAction::AlternateScreen(AlternateScreenControl::EnterLegacy)
                    }
                    (AlternateScreenParameter::Legacy, b'l') => {
                        TerminalControlAction::AlternateScreen(AlternateScreenControl::ExitLegacy)
                    }
                    (AlternateScreenParameter::Native, b'h' | b'l') => {
                        TerminalControlAction::AlternateScreen(AlternateScreenControl::InvalidatePreserved)
                    }
                    (AlternateScreenParameter::Absent, _) => TerminalControlAction::Unchanged,
                    (AlternateScreenParameter::Legacy | AlternateScreenParameter::Native, _) => {
                        TerminalControlAction::Unchanged
                    }
                }
            }
            b'\x18' | b'\x1a' | 0x40..=0x7e => {
                self.state = CursorControlState::Ground;
                TerminalControlAction::Unchanged
            }
            b'\x1b' => {
                self.state = CursorControlState::Escape;
                TerminalControlAction::Unchanged
            }
            _ => {
                self.state = CursorControlState::CsiInvalid;
                TerminalControlAction::Unchanged
            }
        }
    }

    const fn observe_csi_space(&mut self, byte: u8, parameter: CursorParameter) -> TerminalControlAction {
        match byte {
            b'q' => {
                self.state = CursorControlState::Ground;
                match parameter {
                    CursorParameter::Empty | CursorParameter::Value(0) => {
                        TerminalControlAction::Cursor(CursorControl::DefaultShape)
                    }
                    CursorParameter::Value(1..=6) => TerminalControlAction::Cursor(CursorControl::ExplicitShape),
                    CursorParameter::Value(_) | CursorParameter::Invalid => TerminalControlAction::Unchanged,
                }
            }
            0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f => {
                self.state = CursorControlState::CsiSpace(parameter);
                TerminalControlAction::Unchanged
            }
            b'\x18' | b'\x1a' | 0x40..=0x7e => {
                self.state = CursorControlState::Ground;
                TerminalControlAction::Unchanged
            }
            b'\x1b' => {
                self.state = CursorControlState::Escape;
                TerminalControlAction::Unchanged
            }
            _ => {
                self.state = CursorControlState::CsiInvalid;
                TerminalControlAction::Unchanged
            }
        }
    }

    const fn observe_csi_invalid(&mut self, byte: u8) -> TerminalControlAction {
        self.state = match byte {
            b'\x18' | b'\x1a' | 0x40..=0x7e => CursorControlState::Ground,
            b'\x1b' => CursorControlState::Escape,
            _ => CursorControlState::CsiInvalid,
        };
        TerminalControlAction::Unchanged
    }

    fn observe_osc_cursor_shape(&mut self, byte: u8, state: OscCursorShapeState) -> TerminalControlAction {
        match byte {
            b'\x07' | b'\x18' | b'\x1a' => {
                self.state = CursorControlState::Ground;
                TerminalControlAction::Cursor(state.cursor_control())
            }
            b'\x1b' => {
                self.state = CursorControlState::Escape;
                TerminalControlAction::Cursor(state.cursor_control())
            }
            0x00..=0x06 | 0x08..=0x17 | 0x19 | 0x1c..=0x1f => {
                self.state = CursorControlState::OscCursorShape(state);
                TerminalControlAction::Unchanged
            }
            _ => {
                self.state = CursorControlState::OscCursorShape(state.observe(byte));
                TerminalControlAction::Unchanged
            }
        }
    }

    fn append_cursor_parameter(parameter: CursorParameter, byte: u8) -> CursorParameter {
        let digit = u16::from(byte.saturating_sub(b'0'));
        match parameter {
            CursorParameter::Empty => CursorParameter::Value(digit),
            CursorParameter::Value(value) => value
                .checked_mul(10)
                .and_then(|value| value.checked_add(digit))
                .map_or(CursorParameter::Invalid, CursorParameter::Value),
            CursorParameter::Invalid => CursorParameter::Invalid,
        }
    }
}

pub struct TerminalState {
    input_filter: RioInputFilter,
    terminal_control_tracker: TerminalControlTracker,
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
            terminal_control_tracker: TerminalControlTracker::default(),
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
        let terminal_control = self.terminal_control_tracker.process(bytes);
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
            .map(|row| self::row_wrap(&terminal.grid[self::visible_line(terminal, row)]))
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

    #[cfg(test)]
    pub fn snapshot(&self) -> rootcause::Result<TerminalSnapshot> {
        self.snapshot_rows(TerminalSnapshotScope::Full)
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
            let line = visible_line(terminal, usize::from(row.row()));
            terminal.grid[line].dirty = false;
        }
        terminal.reset_damage();
        self.rendered_viewport = Some(rendered_viewport);
        Ok(snapshot)
    }

    fn snapshot_rows(&self, scope: TerminalSnapshotScope) -> rootcause::Result<TerminalSnapshot> {
        let terminal = self.rio.terminal();
        let screen_rows = u16::try_from(terminal.screen_lines()).context("muxr terminal row count overflowed")?;
        let screen_cols = u16::try_from(terminal.columns()).context("muxr terminal column count overflowed")?;
        let size = TerminalSize::new(screen_cols, screen_rows)?;
        let rio_cursor = terminal.cursor();
        let cursor_row = u16::try_from(rio_cursor.pos.row.0).context("muxr terminal cursor row overflowed")?;
        let cursor_col = u16::try_from(rio_cursor.pos.col.0).context("muxr terminal cursor column overflowed")?;
        let cursor_visible = terminal.display_offset() == 0
            && rio_cursor.is_visible()
            && cursor_row < screen_rows
            && cursor_col < screen_cols;
        let cursor = RenderCursor {
            row: cursor_row,
            col: cursor_col,
            shape: self::render_cursor_shape(rio_cursor.content, terminal.blinking_cursor, self.cursor_shape_source),
            visibility: if cursor_visible {
                muxr_core::RenderCursorVisibility::Visible
            } else {
                muxr_core::RenderCursorVisibility::Hidden
            },
        };
        let row_wraps = (0..terminal.screen_lines())
            .map(|row| self::row_wrap(&terminal.grid[self::visible_line(terminal, row)]))
            .collect();
        let mut hyperlink_cache = HashMap::new();
        let rows = (0..terminal.screen_lines())
            .filter(|row| {
                matches!(scope, TerminalSnapshotScope::Full) || terminal.grid[visible_line(terminal, *row)].dirty
            })
            .map(|row| {
                let line = self::visible_line(terminal, row);
                RenderRowSpan::new(
                    u16::try_from(row).context("muxr terminal snapshot row index overflowed")?,
                    0,
                    self::render_row(&terminal.grid, line, usize::from(screen_cols), &mut hyperlink_cache),
                )
            })
            .collect::<rootcause::Result<Vec<_>>>()?;

        Ok(TerminalSnapshot {
            cursor,
            row_wraps,
            rows,
            scope,
            size,
        })
    }

    pub fn scrollback_dump(&mut self, style: ScrollbackDumpStyle) -> Vec<u8> {
        let terminal = self.rio.terminal();
        let history = i32::try_from(terminal.grid.history_size()).unwrap_or(i32::MAX);
        let screen_lines = i32::try_from(terminal.screen_lines()).unwrap_or(i32::MAX);
        if !terminal.mode().contains(Mode::ALT_SCREEN) {
            return self::scrollback_grid_dump(terminal, history.saturating_neg()..screen_lines, style);
        }

        let alternate_dump = self::scrollback_grid_dump(terminal, 0..screen_lines, style);
        let mut dump = self.rio.with_primary_screen(|primary| {
            let history = i32::try_from(primary.grid.history_size()).unwrap_or(i32::MAX);
            self::scrollback_grid_dump(primary, history.saturating_neg()..0, style)
        });
        dump.extend_from_slice(&alternate_dump);
        dump
    }

    #[cfg(test)]
    fn total_scrollback_len(&self) -> usize {
        self.rio.terminal().grid.history_size()
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

fn visible_line<U: EventListener>(terminal: &Crosswords<U>, row: usize) -> Line {
    let row = i32::try_from(row).unwrap_or(i32::MAX);
    let offset = i32::try_from(terminal.display_offset()).unwrap_or(i32::MAX);
    Line(row.saturating_sub(offset))
}

fn scrollback_grid_dump<U: EventListener>(
    terminal: &Crosswords<U>,
    lines: std::ops::Range<i32>,
    style: ScrollbackDumpStyle,
) -> Vec<u8> {
    let mut dump = Vec::new();
    let mut hyperlink_cache = HashMap::new();
    for line in lines {
        let row = self::render_row(&terminal.grid, Line(line), terminal.columns(), &mut hyperlink_cache);
        self::append_scrollback_dump_row(&row, style, &mut dump);
    }
    dump
}

fn row_wrap(row: &Row<Square>) -> RowWrap {
    if row
        .inner
        .last()
        .is_some_and(|square| square.contains_cell_flag(CellFlags::WRAPLINE))
    {
        RowWrap::EndsWithSoftWrap
    } else {
        RowWrap::EndsBeforeSoftWrap
    }
}

fn render_row(
    grid: &Grid<Square>,
    line: Line,
    columns: usize,
    hyperlink_cache: &mut HashMap<ExtrasId, Option<RenderHyperlink>>,
) -> Vec<RenderCell> {
    let row = &grid[line];
    (0..columns)
        .map(|col| {
            row.inner.get(col).map_or_else(
                || RenderCell::narrow(" ", RenderStyle::default()),
                |square| self::render_cell(grid, line, Column(col), *square, hyperlink_cache),
            )
        })
        .collect()
}

fn render_cell(
    grid: &Grid<Square>,
    line: Line,
    col: Column,
    square: Square,
    hyperlink_cache: &mut HashMap<ExtrasId, Option<RenderHyperlink>>,
) -> RenderCell {
    let style = self::render_style(grid, square);
    let width = square.wide();
    let mut cell = match width {
        Wide::Spacer => RenderCell::wide_continuation(style),
        Wide::Wide | Wide::LeadingSpacer | Wide::Narrow if square.is_bg_only() => {
            self::render_text_cell(width, " ", style)
        }
        Wide::Wide | Wide::LeadingSpacer | Wide::Narrow if square.has_grapheme() => {
            let text = self::square_text(grid, line, col);
            self::render_text_cell(
                width,
                std::str::from_utf8(text.as_slice()).map_or(" ", |text| text),
                style,
            )
        }
        Wide::Wide | Wide::LeadingSpacer | Wide::Narrow => {
            let character = normalized_render_character(square.c());
            let mut encoded = [0_u8; char::MAX_LEN_UTF8];
            self::render_text_cell(width, character.encode_utf8(&mut encoded), style)
        }
    };
    if square.content_tag() == ContentTag::Codepoint {
        let hyperlink = square.extras_id().and_then(|id| {
            grid.extras_table
                .get(id)
                .and_then(|extras| extras.hyperlink.as_ref())
                .map(|hyperlink| (id, hyperlink))
        });
        if let Some((id, hyperlink)) = hyperlink {
            let render_hyperlink = hyperlink_cache
                .entry(id)
                .or_insert_with(|| RenderHyperlink::new(hyperlink.uri().to_owned()).ok());
            if let Some(render_hyperlink) = render_hyperlink {
                cell = cell.with_hyperlink(render_hyperlink.clone());
            }
        }
    }
    cell
}

fn render_text_cell(width: Wide, text: &str, style: RenderStyle) -> RenderCell {
    match width {
        Wide::Wide => RenderCell::wide(text, style),
        Wide::Spacer => RenderCell::wide_continuation(style),
        Wide::LeadingSpacer | Wide::Narrow => RenderCell::narrow(text, style),
    }
}

fn square_text(grid: &Grid<Square>, line: Line, col: Column) -> SmallVec<[u8; RENDER_CELL_TEXT_INLINE_BYTES]> {
    let mut text = SmallVec::new();
    for character in grid.cell_text(Pos::new(line, col)) {
        let character = self::normalized_render_character(character);
        let mut encoded = [0_u8; char::MAX_LEN_UTF8];
        text.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
    }
    if text.is_empty() {
        text.push(b' ');
    }
    text
}

const fn normalized_render_character(character: char) -> char {
    if matches!(character, '\0' | '\t') {
        ' '
    } else {
        character
    }
}

fn render_style(grid: &Grid<Square>, square: Square) -> RenderStyle {
    if square.is_bg_only() {
        return RenderStyle {
            attrs: RenderTextStyle::empty(),
            bg: match square.content_tag() {
                ContentTag::BgPalette => RenderColor::Indexed(square.bg_palette_index()),
                ContentTag::BgRgb => {
                    let (r, g, b) = square.bg_rgb();
                    RenderColor::Rgb { r, g, b }
                }
                ContentTag::Codepoint => RenderColor::Default,
            },
            fg: RenderColor::Default,
        };
    }
    let style = grid.style_of(&square);
    RenderStyle {
        attrs: RenderTextStyle::empty()
            .set_bold(style.flags.contains(StyleFlags::BOLD))
            .set_dim(style.flags.contains(StyleFlags::DIM))
            .set_italic(style.flags.contains(StyleFlags::ITALIC))
            .set_underline(style.flags.intersects(StyleFlags::ALL_UNDERLINES))
            .set_inverse(style.flags.contains(StyleFlags::INVERSE)),
        bg: self::render_color(style.bg),
        fg: self::render_color(style.fg),
    }
}

fn render_color(color: AnsiColor) -> RenderColor {
    match color {
        AnsiColor::Indexed(index) => RenderColor::Indexed(index),
        AnsiColor::Spec(rgb) => RenderColor::Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        },
        AnsiColor::Named(named) => self::render_named_color(named),
    }
}

fn render_named_color(color: NamedColor) -> RenderColor {
    let index = color as u16;
    u8::try_from(index)
        .ok()
        .filter(|index| *index <= 15)
        .map_or(RenderColor::Default, RenderColor::Indexed)
}

const fn render_cursor_shape(shape: RioCursorShape, blinking: bool, source: CursorShapeSource) -> RenderCursorShape {
    match source {
        CursorShapeSource::Default => RenderCursorShape::Default,
        CursorShapeSource::Explicit => match (shape, blinking) {
            (RioCursorShape::Beam, false) => RenderCursorShape::SteadyBar,
            (RioCursorShape::Beam, true) => RenderCursorShape::BlinkingBar,
            (RioCursorShape::Block, false) => RenderCursorShape::SteadyBlock,
            (RioCursorShape::Block, true) => RenderCursorShape::BlinkingBlock,
            (RioCursorShape::Underline, false) => RenderCursorShape::SteadyUnderline,
            (RioCursorShape::Underline, true) => RenderCursorShape::BlinkingUnderline,
            (RioCursorShape::Hidden, _) => RenderCursorShape::Default,
        },
    }
}

fn append_scrollback_dump_row(row: &[RenderCell], style: ScrollbackDumpStyle, bytes: &mut Vec<u8>) {
    match style {
        ScrollbackDumpStyle::PlainText => self::encode_plain_scrollback_dump_row(row, bytes),
        ScrollbackDumpStyle::Ansi => self::encode_ansi_scrollback_dump_row(row, bytes),
    }
    bytes.push(b'\n');
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
    use test_that::prelude::*;

    use super::*;

    fn assert_replies_eq(replies: &TerminalReplies, expected: &[Vec<u8>]) {
        assert_that!(replies.as_slice(), eq(expected));
    }

    #[test]
    fn test_paste_input_bytes_when_bracketed_paste_is_enabled_wraps_payload() {
        assert_that!(
            paste_input_bytes(b"one\ntwo\n", TerminalPasteMode::Bracketed),
            eq(b"\x1b[200~one\ntwo\n\x1b[201~".to_vec())
        );
    }

    #[test]
    fn test_paste_input_bytes_when_bracketed_paste_is_disabled_preserves_payload() {
        assert_that!(
            paste_input_bytes(b"one\ntwo\n", TerminalPasteMode::Plain),
            eq(b"one\ntwo\n".to_vec())
        );
    }

    #[test]
    fn test_terminal_state_snapshot_when_output_processed_contains_screen() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let outcome = terminal.process(b"hi");
        self::assert_replies_eq(&outcome.into_replies(), &[]);
        let snapshot = terminal.snapshot()?;
        let Some(row) = snapshot.rows().first() else {
            return Err(report!("expected first render row"));
        };
        let rendered = row.cells().iter().take(2).map(RenderCell::text).collect::<String>();

        assert_that!(rendered, eq("hi"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_snapshot_when_osc8_span_is_rendered_shares_uri_allocation() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 1)?);
        let _outcome = terminal.process(b"\x1b]8;;https://example.com\x07click\x1b]8;;\x07");
        let snapshot = terminal.snapshot()?;
        let Some(row) = snapshot.rows().first() else {
            return Err(report!("expected first render row"));
        };
        let mut hyperlinks = row.cells().iter().filter_map(RenderCell::hyperlink);
        let Some(first) = hyperlinks.next() else {
            return Err(report!("expected first rendered hyperlink"));
        };
        let Some(second) = hyperlinks.next() else {
            return Err(report!("expected second rendered hyperlink"));
        };

        assert_that!(std::ptr::eq(first.uri(), second.uri()), eq(true));
        Ok(())
    }

    #[test]
    fn terminal_state_snapshot_when_output_contains_horizontal_tab_expands_it_to_spaces() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(16, 1)?);

        let _outcome = terminal.process(b"\tmodified");
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert_that!(rendered, starts_with("        modified"));
        Ok(())
    }

    #[test]
    fn terminal_state_snapshot_when_combining_mark_precedes_horizontal_tab_expands_tab_to_spaces()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(16, 1)?);

        let _outcome = terminal.process("\u{301}\tmodified".as_bytes());
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert_that!(rendered, not(contains_substring("\t")));
        assert_that!(rendered, starts_with(" \u{301}       modified"));
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

        self::assert_replies_eq(&terminal.process(bytes).into_replies(), &[expected.to_vec()]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_cursor_report_requested_returns_current_cursor() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(&terminal.process(b"\x1b[2;3H").into_replies(), &[]);

        self::assert_replies_eq(&terminal.process(b"\x1b[6n").into_replies(), &[b"\x1b[2;3R".to_vec()]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_snapshot_when_cursor_shape_is_set_returns_shape() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(&terminal.process(b"\x1b[6 q").into_replies(), &[]);

        assert_that!(terminal.snapshot()?.cursor().shape, eq(RenderCursorShape::SteadyBar));
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_only_cursor_shape_changes_marks_screen_dirty() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let outcome = terminal.process(b"\x1b[6 q");

        assert_that!(outcome.screen_dmg(), eq(TerminalScreenDmg::Dirty));
        Ok(())
    }

    #[rstest]
    #[case::cursor_visibility(b"\x1b[?25l")]
    #[case::mouse_protocol(b"\x1b[?1002h\x1b[?1006h")]
    fn test_terminal_state_process_when_only_render_metadata_changes_marks_render_dirty(
        #[case] bytes: &[u8],
    ) -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let outcome = terminal.process(bytes);

        assert_that!(outcome.screen_dmg(), eq(TerminalScreenDmg::Dirty));
        Ok(())
    }

    #[test]
    fn test_terminal_state_snapshot_when_steady_block_is_explicit_returns_explicit_shape() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _outcome = terminal.process(b"\x1b[2 q");

        assert_that!(terminal.snapshot()?.cursor().shape, eq(RenderCursorShape::SteadyBlock));
        Ok(())
    }

    #[test]
    fn test_terminal_state_snapshot_when_osc_50_sets_initial_block_returns_explicit_shape() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let outcome = terminal.process(b"\x1b]50;CursorShape=0\x07");

        assert_that!(outcome.screen_dmg(), eq(TerminalScreenDmg::Dirty));
        assert_that!(terminal.snapshot()?.cursor().shape, eq(RenderCursorShape::SteadyBlock));
        Ok(())
    }

    #[test]
    fn test_terminal_state_snapshot_when_osc_50_changes_beam_to_block_returns_explicit_block() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&terminal_size()?);
        let _beam = terminal.process(b"\x1b]50;CursorShape=1\x07");
        assert_that!(terminal.snapshot()?.cursor().shape, eq(RenderCursorShape::SteadyBar));

        let block = terminal.process(b"\x1b]50;CursorShape=0\x07");

        assert_that!(block.screen_dmg(), eq(TerminalScreenDmg::Dirty));
        assert_that!(terminal.snapshot()?.cursor().shape, eq(RenderCursorShape::SteadyBlock));
        Ok(())
    }

    #[test]
    fn test_terminal_state_snapshot_when_osc_50_sequence_is_split_returns_explicit_shape() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let first = terminal.process(b"\x1b]50;Cursor");
        let second = terminal.process(b"Shape=0\x1b\\");

        assert_that!(first.screen_dmg(), eq(TerminalScreenDmg::Clean));
        assert_that!(second.screen_dmg(), eq(TerminalScreenDmg::Dirty));
        assert_that!(terminal.snapshot()?.cursor().shape, eq(RenderCursorShape::SteadyBlock));
        Ok(())
    }

    #[test]
    fn test_terminal_state_snapshot_when_cursor_shape_sequence_is_split_returns_shape() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _first = terminal.process(b"\x1b[2 ");
        let second = terminal.process(b"q");

        assert_that!(second.screen_dmg(), eq(TerminalScreenDmg::Dirty));
        assert_that!(terminal.snapshot()?.cursor().shape, eq(RenderCursorShape::SteadyBlock));
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_utf8_contains_c1_byte_preserves_text_and_screen_mode() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _outcome = terminal.process("ś?47h".as_bytes());

        assert_that!(terminal.application_mode().screen_mode, eq(TerminalScreenMode::Normal));
        assert_that!(self::snapshot_text(&terminal.snapshot()?), contains_substring("ś?47h"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_utf8_c1_byte_is_split_preserves_text_and_screen_mode() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _first = terminal.process(b"\xc5");
        let _second = terminal.process(b"\x9b?47h");

        assert_that!(terminal.application_mode().screen_mode, eq(TerminalScreenMode::Normal));
        assert_that!(self::snapshot_text(&terminal.snapshot()?), contains_substring("ś?47h"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_snapshot_when_terminal_resets_clears_cursor_shape() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(&terminal.process(b"\x1b[6 q\x1bc").into_replies(), &[]);

        assert_that!(terminal.snapshot()?.cursor().shape, eq(RenderCursorShape::Default));
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_report_sequence_is_split_returns_one_reply() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(&terminal.process(b"\x1b[").into_replies(), &[]);

        self::assert_replies_eq(&terminal.process(b"6n").into_replies(), &[b"\x1b[1;1R".to_vec()]);
        Ok(())
    }

    #[rstest]
    #[case::osc_zero(b"\x1b]0;cargo test\x07")]
    #[case::osc_two(b"\x1b]2;cargo test\x07")]
    fn test_terminal_state_title_when_window_title_is_set_returns_title(#[case] bytes: &[u8]) -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(&terminal.process(bytes).into_replies(), &[]);

        assert_that!(terminal.title(), eq(Some("cargo test".to_owned())));
        Ok(())
    }

    #[test]
    fn test_terminal_state_take_title_changes_when_window_title_changes_returns_once() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(&terminal.process(b"\x1b]2;cargo test\x07").into_replies(), &[]);

        assert_that!(terminal.take_title_changes(), eq(vec![Some("cargo test".to_owned())]));
        assert_that!(terminal.take_title_changes(), eq(Vec::<Option<String>>::new()));
        Ok(())
    }

    #[test]
    fn test_terminal_state_take_title_changes_when_window_title_repeats_returns_empty() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(&terminal.process(b"\x1b]2;cargo test\x07").into_replies(), &[]);
        assert_that!(terminal.take_title_changes(), eq(vec![Some("cargo test".to_owned())]));
        self::assert_replies_eq(&terminal.process(b"\x1b]2;cargo test\x07").into_replies(), &[]);

        assert_that!(terminal.take_title_changes(), eq(Vec::<Option<String>>::new()));
        Ok(())
    }

    #[test]
    fn test_terminal_state_take_title_changes_when_titles_change_in_one_chunk_preserves_order() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(&terminal.process(b"\x1b]2;gst\x07\x1b]2;~\x07").into_replies(), &[]);

        assert_that!(
            terminal.take_title_changes(),
            eq(vec![Some("gst".to_owned()), Some("~".to_owned())])
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_title_when_window_title_sequence_is_split_returns_title() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(&terminal.process(b"\x1b]2;").into_replies(), &[]);
        self::assert_replies_eq(&terminal.process(b"gst\x07").into_replies(), &[]);

        assert_that!(terminal.title(), eq(Some("gst".to_owned())));
        Ok(())
    }

    #[rstest]
    #[case::can(b'\x18')]
    #[case::sub(b'\x1a')]
    fn test_terminal_state_title_when_split_window_title_is_canceled_remains_unchanged(
        #[case] cancel: u8,
    ) -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);
        let _ = terminal.process(b"\x1b]2;before\x07");
        let _changes = terminal.take_title_changes();

        let _ = terminal.process(b"\x1b]2;after");
        let _ = terminal.process(&[cancel]);

        assert_that!(terminal.title(), eq(Some("before".to_owned())));
        assert_that!(terminal.take_title_changes(), eq(Vec::<Option<String>>::new()));
        Ok(())
    }

    #[test]
    fn test_terminal_state_title_when_window_title_is_empty_returns_none() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(b"\x1b]2;cargo test\x07");
        let _ = terminal.process(b"\x1b]2;  \x07");

        assert_that!(terminal.title(), eq(None));
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

        assert_that!(outcome.screen_dmg(), eq(TerminalScreenDmg::Clean));
        self::assert_replies_eq(&outcome.into_replies(), &[]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_title_sequence_is_split_keeps_screen_clean() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let first = terminal.process(b"\x1b]2;");
        let second = terminal.process(b"gst\x07");

        assert_that!(first.screen_dmg(), eq(TerminalScreenDmg::Clean));
        self::assert_replies_eq(&first.into_replies(), &[]);
        assert_that!(second.screen_dmg(), eq(TerminalScreenDmg::Clean));
        self::assert_replies_eq(&second.into_replies(), &[]);
        assert_that!(terminal.title(), eq(Some("gst".to_owned())));
        Ok(())
    }

    #[rstest]
    #[case::text(b"hi")]
    #[case::title_then_text(b"\x1b]2;gst\x07hi")]
    #[case::canceled_title_then_text(b"\x1b]2;gst\x18hi")]
    fn test_terminal_state_process_when_output_is_not_title_only_marks_screen_dirty(
        #[case] bytes: &[u8],
    ) -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        assert_that!(terminal.process(bytes).screen_dmg(), eq(TerminalScreenDmg::Dirty));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_output_exceeds_viewport_shows_history() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"one\ntwo\nthree");

        assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
        let rendered = self::snapshot_text(&terminal.snapshot()?);
        assert_that!(rendered, contains_substring("one"));
        Ok(())
    }

    #[test]
    fn terminal_state_snapshot_when_viewport_is_scrolled_hides_live_cursor() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);
        let _output = terminal.process(b"one\r\ntwo\r\nthree");

        assert_that!(
            terminal.scroll_one_line(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Moved)
        );
        assert_that!(
            terminal.snapshot()?.cursor().visibility,
            eq(muxr_core::RenderCursorVisibility::Hidden)
        );

        assert_that!(terminal.scroll_to_bottom(), eq(TerminalScrollMove::Moved));
        assert_that!(
            terminal.snapshot()?.cursor().visibility,
            eq(muxr_core::RenderCursorVisibility::Visible)
        );
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

        assert_that!(
            terminal.scroll_one_line(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Moved)
        );
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert_that!(rendered, contains_substring("row-16"));
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
            let _movement = terminal.scroll_one_line(PaneScrollDirection::Up);
        }
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert_that!(rendered, contains_substring("row-00"));
        assert_that!(rendered, contains_substring("row-03"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_snapshot_when_scrolled_after_narrowing_resize_fits_history_to_current_width()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);
        let _ = terminal.process(b"one\r\ntwo\r\nthree");
        assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));

        let before = terminal.snapshot()?;
        let before_widths = before
            .rows()
            .iter()
            .map(RenderRowSpan::width)
            .collect::<rootcause::Result<Vec<_>>>()?;
        assert_that!(before_widths, eq(vec![8, 8]));

        terminal.resize(&TerminalSize::new(4, 2)?);
        let after = terminal.snapshot()?;
        let after_widths = after
            .rows()
            .iter()
            .map(RenderRowSpan::width)
            .collect::<rootcause::Result<Vec<_>>>()?;
        assert_that!(after_widths, eq(vec![4, 4]));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_output_wraps_preserves_all_visual_rows() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"before\r\n");
        let _ = terminal.process(b"|abcdefghij|\r\n|klmnopqrst|\r\npsql> ");

        let mut rendered = Vec::new();
        rendered.push(self::snapshot_text(&terminal.snapshot()?));
        while terminal.scroll_one_line(PaneScrollDirection::Up) == TerminalScrollMove::Moved {
            rendered.push(self::snapshot_text(&terminal.snapshot()?));
        }
        let rendered = rendered.join("\n");

        assert_that!(rendered, contains_substring("|abcdefg"));
        assert_that!(rendered, contains_substring("hij|"));
        assert_that!(rendered, contains_substring("|klmnopq"));
        assert_that!(rendered, contains_substring("rst|"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_bottom_right_cell_is_filled_waits_for_next_printable_to_scroll()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(4, 2)?);

        let _ = terminal.process(b"top\r\nabc");
        let _ = terminal.process(b"d");

        assert_that!(
            terminal.scroll_one_line(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Unchanged)
        );

        let _ = terminal.process(b"e");

        assert_that!(
            terminal.scroll_one_line(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Moved)
        );
        let rendered = self::snapshot_text(&terminal.snapshot()?);
        assert_that!(rendered, contains_substring("top"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_wide_printable_wraps_preserves_scrolled_row() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(4, 2)?);

        let _ = terminal.process(b"top\r\nabc");
        let _ = terminal.process("字".as_bytes());

        assert_that!(
            terminal.scroll_one_line(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Moved)
        );
        let rendered = self::snapshot_text(&terminal.snapshot()?);
        assert_that!(rendered, contains_substring("top"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_alternate_screen_sets_partial_region_preserves_normal_scrollback()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 3)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[2;3r\x1b[r\x1b[?1049l");
        let _ = terminal.process(b"one\r\ntwo\r\nthree\r\nfour");

        assert_that!(
            terminal.scroll_one_line(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Moved)
        );
        let rendered = self::snapshot_text(&terminal.snapshot()?);
        assert_that!(rendered, contains_substring("one"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scrollback_dump_when_partial_history_precedes_normal_output_keeps_chronology()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(16, 3)?);

        let _ = terminal.process(b"partial-old\r\npartial-live");
        let _ = terminal.process(b"\x1b[1;2r\x1b[2;1H\x1b[S\x1b[r");
        let _ = terminal.process(b"\x1b[3;1Hnormal-new-1\r\nnormal-new-2\r\nnormal-new-3\r\n");

        let dump = String::from_utf8(self::test_scrollback_dump(
            &mut terminal,
            ScrollbackDumpStyle::PlainText,
        ))?;

        assert_that!(
            dump.find("partial-old").is_some_and(|partial_index| {
                dump.find("normal-new-1")
                    .is_some_and(|normal_index| partial_index < normal_index)
            }),
            eq(true)
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scrollback_dump_when_output_exceeds_viewport_returns_history_and_live_rows()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"one\r\ntwo\r\nthree");

        assert_that!(
            String::from_utf8(self::test_scrollback_dump(
                &mut terminal,
                ScrollbackDumpStyle::PlainText
            ))?,
            eq("one\ntwo\nthree\n")
        );
        Ok(())
    }

    #[test]
    fn terminal_state_scrollback_dump_when_alternate_screen_is_active_includes_normal_history() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);
        let _output = terminal.process(b"one\r\ntwo\r\nthree\x1b[?1049halt");
        let before = terminal.snapshot()?;
        let mode_before = terminal.application_mode();

        let dump = String::from_utf8(self::test_scrollback_dump(
            &mut terminal,
            ScrollbackDumpStyle::PlainText,
        ))?;

        assert_that!(dump.as_str(), contains_substring("one"));
        assert_that!(dump.as_str(), contains_substring("alt"));
        assert_that!(terminal.snapshot()?, eq(before));
        assert_that!(terminal.application_mode(), eq(mode_before));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scrollback_dump_when_viewport_is_scrolled_preserves_viewport() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);
        let _ = terminal.process(b"one\r\ntwo\r\nthree");
        assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
        let before = terminal.snapshot()?;

        let _dump = self::test_scrollback_dump(&mut terminal, ScrollbackDumpStyle::PlainText);
        let after = terminal.snapshot()?;

        assert_that!(after, eq(before));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scrollback_dump_when_top_partial_scroll_region_moves_rows_includes_captured_rows()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[2S\x1b[r");

        assert_that!(
            String::from_utf8(self::test_scrollback_dump(
                &mut terminal,
                ScrollbackDumpStyle::PlainText
            ))?,
            eq("one\ntwo\nthree\n\n\nprompt\n")
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scrollback_dump_when_ansi_style_requested_preserves_rendered_style() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"\x1b[31mred\x1b[0m");

        assert_that!(
            String::from_utf8(self::test_scrollback_dump(&mut terminal, ScrollbackDumpStyle::Ansi))?,
            eq("\x1b[0;38;5;1mred\x1b[0m\n\n")
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_to_bottom_when_scrolled_shows_live_viewport() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"one\ntwo\nthree");
        assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));

        assert_that!(terminal.scroll_to_bottom(), eq(TerminalScrollMove::Moved));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert_that!(rendered, contains_substring("three"));
        assert_that!(terminal.scroll_to_bottom(), eq(TerminalScrollMove::Unchanged));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_top_partial_scroll_region_moves_rows_preserves_history() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[2S\x1b[r");

        assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert_that!(rendered, contains_substring("one"));
        assert_that!(rendered, contains_substring("two"));
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

        assert_that!(terminal.total_scrollback_len(), eq(2));
        let retained_text = String::from_utf8(terminal.scrollback_dump(ScrollbackDumpStyle::PlainText))?;
        assert_that!(retained_text.as_str(), not(contains_substring("row-0")));
        assert_that!(retained_text.as_str(), not(contains_substring("row-1")));
        assert_that!(retained_text.as_str(), contains_substring("row-2"));
        assert_that!(retained_text.as_str(), contains_substring("row-3"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_partial_scroll_sequence_is_split_preserves_history() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[");
        let _ = terminal.process(b"2S\x1b[r");

        assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert_that!(rendered, contains_substring("one"));
        assert_that!(rendered, contains_substring("two"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_top_partial_scroll_region_linefeed_moves_rows_prefers_captured_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"old-0\nold-1\nold-2\nold-3\nold-4\n");
        let _ = terminal.process(b"\x1b[1;1Hcod-0\x1b[2;1Hcod-1\x1b[3;1Hcod-2\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[3;1H\n\x1b[r");

        assert_that!(
            terminal.scroll_one_line(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Moved)
        );
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert_that!(rendered, contains_substring("cod-0"));
        assert_that!(rendered, not(contains_substring("old-")));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_alternate_screen_linefeed_moves_rows_does_not_capture_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[3;1H\n\x1b[r");

        assert_that!(
            terminal.scroll(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Unchanged)
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_top_partial_scroll_region_delete_lines_moves_rows_preserves_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[1;1H\x1b[2M\x1b[r");

        assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert_that!(rendered, contains_substring("one"));
        assert_that!(rendered, contains_substring("two"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_full_scroll_region_delete_lines_moves_rows_preserves_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;4r\x1b[1;1H\x1b[2M\x1b[r");

        assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert_that!(rendered, contains_substring("one"));
        assert_that!(rendered, contains_substring("two"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_mid_partial_scroll_region_delete_lines_moves_rows_does_not_capture_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[2;1H\x1b[2M\x1b[r");

        assert_that!(
            terminal.scroll(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Unchanged)
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_alternate_screen_delete_lines_moves_rows_does_not_capture_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;4r\x1b[1;1H\x1b[2M\x1b[r");

        assert_that!(
            terminal.scroll(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Unchanged)
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_alternate_screen_partial_scroll_region_moves_rows_does_not_capture_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;3r\x1b[2S\x1b[r");

        assert_that!(
            terminal.scroll(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Unchanged)
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_alternate_screen_full_scroll_region_moves_rows_does_not_capture_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;4r\x1b[2S\x1b[r");

        assert_that!(
            terminal.scroll(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Unchanged)
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_scroll_when_normal_screen_full_scroll_region_moves_rows_preserves_history()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

        let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
        let _ = terminal.process(b"\x1b[1;4r\x1b[2S\x1b[r");

        assert_that!(terminal.total_scrollback_len(), eq(2));
        assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
        let rendered = self::snapshot_text(&terminal.snapshot()?);

        assert_that!(rendered, contains_substring("one"));
        assert_that!(rendered, contains_substring("two"));
        Ok(())
    }

    #[test]
    fn test_terminal_state_visible_top_row_when_scrolled_tracks_current_viewport() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

        let _ = terminal.process(b"one\ntwo\nthree");
        let bottom_top_row = terminal.visible_top_row()?;
        assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
        let scrolled_snapshot = self::snapshot_text(&terminal.snapshot()?);

        let scrolled_top_row = terminal.visible_top_row()?;

        assert_that!(self::snapshot_text(&terminal.snapshot()?), eq(scrolled_snapshot));
        assert_that!(scrolled_top_row, lt(bottom_top_row));
        Ok(())
    }

    #[test]
    fn test_terminal_state_visible_row_wraps_when_live_row_wraps_reports_flag() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(4, 2)?);

        let _ = terminal.process(b"abcdx");

        assert_that!(
            terminal.visible_row_wraps(),
            eq(vec![RowWrap::EndsWithSoftWrap, RowWrap::EndsBeforeSoftWrap])
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_bracketed_paste_when_mode_is_enabled_returns_true() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(b"\x1b[?2004h");

        assert_that!(terminal.paste_mode(), eq(TerminalPasteMode::Bracketed));
        Ok(())
    }

    #[test]
    fn test_terminal_state_mouse_protocol_when_sgr_button_motion_is_enabled_returns_protocol() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(b"\x1b[?1002h\x1b[?1006h");

        assert_that!(
            terminal.mouse_protocol(),
            eq(Some(TerminalMouseProtocol {
                mode: TerminalMouseProtocolMode::ButtonMotion,
                encoding: TerminalMouseProtocolEncoding::Sgr
            }))
        );
        assert_that!(
            terminal.application_mode().pane_mouse_mode(),
            eq(PaneMouseMode::ButtonMotion)
        );
        assert_that!(
            terminal.application_mode().pane_mouse_mode(),
            eq(PaneMouseMode::ButtonMotion)
        );
        Ok(())
    }

    #[rstest]
    #[case::alternate_47_enabled(b"\x1b[?47h", TerminalScreenMode::Alternate)]
    #[case::alternate_1049_enabled(b"\x1b[?1049h", TerminalScreenMode::Alternate)]
    #[case::alternate_47_disabled(b"\x1b[?47h\x1b[?47l", TerminalScreenMode::Normal)]
    #[case::alternate_1049_disabled(b"\x1b[?1049h\x1b[?1049l", TerminalScreenMode::Normal)]
    fn test_terminal_state_application_mode_when_alternate_screen_changes_returns_state(
        #[case] bytes: &[u8],
        #[case] expected: TerminalScreenMode,
    ) -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(bytes);

        assert_that!(
            terminal.application_mode(),
            eq(TerminalApplicationMode {
                screen_mode: expected,
                cursor_key_mode: TerminalCursorKeyMode::Normal,
                keyboard_protocol: TerminalKeyboardProtocol::Legacy,
                focus_reporting: TerminalFocusReporting::Disabled,
                mouse_protocol: None,
            })
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_application_mode_when_legacy_alternate_screen_sequence_is_split_returns_state()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(b"\x1b[?4");
        assert_that!(terminal.application_mode().screen_mode, eq(TerminalScreenMode::Normal));
        let _ = terminal.process(b"7h");
        assert_that!(
            terminal.application_mode().screen_mode,
            eq(TerminalScreenMode::Alternate)
        );
        let _ = terminal.process(b"\x1b[?47");
        let _ = terminal.process(b"l");

        assert_that!(terminal.application_mode().screen_mode, eq(TerminalScreenMode::Normal));
        Ok(())
    }

    #[rstest]
    #[case::bell(b"\x07")]
    #[case::delete(b"\x7f")]
    fn test_terminal_state_application_mode_when_csi_control_is_embedded_keeps_parsing_legacy_alternate_screen(
        #[case] control: &[u8],
    ) -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);
        let mut sequence = b"\x1b[?4".to_vec();
        sequence.extend_from_slice(control);
        sequence.extend_from_slice(b"7h");

        let _outcome = terminal.process(&sequence);

        assert_that!(
            terminal.application_mode().screen_mode,
            eq(TerminalScreenMode::Alternate)
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_application_mode_when_csi_control_precedes_chunk_boundary_keeps_parsing_legacy_alternate_screen()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _first = terminal.process(b"\x1b[?4\x07");
        let _second = terminal.process(b"7h");

        assert_that!(
            terminal.application_mode().screen_mode,
            eq(TerminalScreenMode::Alternate)
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_application_mode_when_legacy_alternate_screen_is_grouped_returns_state()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _enabled = terminal.process(b"\x1b[?1;47h");
        assert_that!(
            terminal.application_mode().screen_mode,
            eq(TerminalScreenMode::Alternate)
        );
        assert_that!(
            terminal.application_mode().cursor_key_mode,
            eq(TerminalCursorKeyMode::Application)
        );

        let _disabled = terminal.process(b"\x1b[?1;47l");
        assert_that!(terminal.application_mode().screen_mode, eq(TerminalScreenMode::Normal));
        assert_that!(
            terminal.application_mode().cursor_key_mode,
            eq(TerminalCursorKeyMode::Normal)
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_legacy_alternate_screen_wraps_content_keeps_grids_separate()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);

        let _ = terminal.process(b"normal\x1b[?47halt");

        assert_that!(self::snapshot_text(&terminal.snapshot()?), contains_substring("alt"));
        assert_that!(
            self::snapshot_text(&terminal.snapshot()?),
            not(contains_substring("normal"))
        );

        let _ = terminal.process(b"\x1b[?47l");

        assert_that!(self::snapshot_text(&terminal.snapshot()?), contains_substring("normal"));
        assert_that!(
            self::snapshot_text(&terminal.snapshot()?),
            not(contains_substring("alt"))
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_legacy_alternate_screen_is_reentered_preserves_contents()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);

        let _ = terminal.process(b"normal\x1b[?47halt\x1b[?47l\x1b[?47h");

        assert_that!(self::snapshot_text(&terminal.snapshot()?), contains_substring("alt"));
        assert_that!(
            self::snapshot_text(&terminal.snapshot()?),
            not(contains_substring("normal"))
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_reset_precedes_legacy_alternate_reentry_does_not_restore_stale_contents()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);
        let _setup = terminal.process(b"normal\x1b[?47hlegacy\x1b[?47l");

        let _reset_and_reenter = terminal.process(b"\x1bc\x1b[?47h");

        assert_that!(
            self::snapshot_text(&terminal.snapshot()?),
            not(contains_substring("legacy"))
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_native_alternate_cycle_is_buffered_does_not_restore_older_legacy_contents()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);
        let _legacy = terminal.process(b"normal\x1b[?47hlegacy\x1b[?47l");

        let _native_cycle = terminal.process(b"\x1b[?1049hnative\x1b[?1049l");
        let _legacy_reentry = terminal.process(b"\x1b[?47h");

        assert_that!(
            self::snapshot_text(&terminal.snapshot()?),
            not(contains_substring("legacy"))
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_c1_legacy_alternate_screen_is_used_keeps_grids_separate()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);

        let _alternate = terminal.process(b"normal\x9b?47halt");

        assert_that!(self::snapshot_text(&terminal.snapshot()?), contains_substring("alt"));
        assert_that!(
            self::snapshot_text(&terminal.snapshot()?),
            not(contains_substring("normal"))
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_c1_native_alternate_cycle_is_buffered_does_not_restore_older_legacy_contents()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);
        let _legacy = terminal.process(b"normal\x1b[?47hlegacy\x1b[?47l");

        let _native_cycle = terminal.process(b"\x9b?1049hnative\x9b?1049l");
        let _legacy_reentry = terminal.process(b"\x1b[?47h");

        assert_that!(
            self::snapshot_text(&terminal.snapshot()?),
            not(contains_substring("legacy"))
        );
        Ok(())
    }

    #[rstest]
    #[case::application_cursor_enabled(b"\x1b[?1h", TerminalCursorKeyMode::Application)]
    #[case::application_cursor_disabled(b"\x1b[?1h\x1b[?1l", TerminalCursorKeyMode::Normal)]
    fn test_terminal_state_application_mode_when_application_cursor_changes_returns_state(
        #[case] bytes: &[u8],
        #[case] expected: TerminalCursorKeyMode,
    ) -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(bytes);

        assert_that!(
            terminal.application_mode(),
            eq(TerminalApplicationMode {
                screen_mode: TerminalScreenMode::Normal,
                cursor_key_mode: expected,
                keyboard_protocol: TerminalKeyboardProtocol::Legacy,
                focus_reporting: TerminalFocusReporting::Disabled,
                mouse_protocol: None,
            })
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

        assert_that!(
            terminal.application_mode(),
            eq(TerminalApplicationMode {
                screen_mode: TerminalScreenMode::Normal,
                cursor_key_mode: if bytes == b"\x1b[?1;1004h" {
                    TerminalCursorKeyMode::Application
                } else {
                    TerminalCursorKeyMode::Normal
                },
                keyboard_protocol: TerminalKeyboardProtocol::Legacy,
                focus_reporting: expected,
                mouse_protocol: None,
            })
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

        assert_that!(terminal.application_mode().keyboard_protocol, eq(expected));
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_keyboard_protocol_is_queried_returns_status() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(
            &terminal.process(b"\x1b[?u").into_replies(),
            &[KITTY_KEYBOARD_PROTOCOL_DISABLED_REPLY.to_vec()],
        );
        let _ = terminal.process(b"\x1b[>1u");
        self::assert_replies_eq(
            &terminal.process(b"\x1b[?u").into_replies(),
            &[KITTY_KEYBOARD_PROTOCOL_ENABLED_REPLY.to_vec()],
        );
        let _ = terminal.process(b"\x1b[<u");
        self::assert_replies_eq(
            &terminal.process(b"\x1b[?u").into_replies(),
            &[KITTY_KEYBOARD_PROTOCOL_DISABLED_REPLY.to_vec()],
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_application_mode_when_terminal_reset_sequence_is_split_clears_focus_reporting()
    -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(b"\x1b[?1004h\x1b");
        let _ = terminal.process(b"c");

        assert_that!(
            terminal.application_mode().focus_reporting,
            eq(TerminalFocusReporting::Disabled)
        );
        Ok(())
    }

    #[test]
    fn test_terminal_state_application_mode_when_mouse_protocol_is_enabled_returns_protocol() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        let _ = terminal.process(b"\x1b[?1002h\x1b[?1006h");

        assert_that!(
            terminal.application_mode(),
            eq(TerminalApplicationMode {
                screen_mode: TerminalScreenMode::Normal,
                cursor_key_mode: TerminalCursorKeyMode::Normal,
                keyboard_protocol: TerminalKeyboardProtocol::Legacy,
                focus_reporting: TerminalFocusReporting::Disabled,
                mouse_protocol: Some(TerminalMouseProtocol {
                    mode: TerminalMouseProtocolMode::ButtonMotion,
                    encoding: TerminalMouseProtocolEncoding::Sgr,
                }),
            })
        );
        Ok(())
    }

    #[rstest]
    #[case::private_cursor_report(b"\x1b[?6n")]
    #[case::unknown_report(b"\x1b[9n")]
    fn test_terminal_state_process_when_report_is_unsupported_returns_no_reply(
        #[case] bytes: &[u8],
    ) -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(&terminal.process(bytes).into_replies(), &[]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_process_when_multi_param_status_report_requested_returns_rio_reply() -> rootcause::Result<()>
    {
        let mut terminal = self::terminal_state(&terminal_size()?);

        self::assert_replies_eq(&terminal.process(b"\x1b[5;6n").into_replies(), &[b"\x1b[0n".to_vec()]);
        Ok(())
    }

    #[test]
    fn test_terminal_state_render_snapshot_when_one_row_changes_returns_only_that_row() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);
        let baseline = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
        assert_that!(baseline.rows().len(), eq(usize::from(terminal_size()?.rows())));

        let _outcome = terminal.process(b"\x1b[2;1Hchanged");
        let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;

        assert_that!(
            update.rows().iter().map(RenderRowSpan::row).collect::<Vec<_>>(),
            eq(vec![1])
        );
        Ok(())
    }

    #[test]
    fn terminal_state_render_snapshot_when_live_partial_region_scrolls_returns_only_region_rows()
    -> rootcause::Result<()> {
        let size = TerminalSize::new(8, 4)?;
        let mut terminal = self::terminal_state(&size);
        let _setup = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hfixed-a\x1b[4;1Hfixed-b\x1b[1;2r");
        let mut cached = terminal.render_snapshot(TerminalSnapshotScope::Full)?;

        let _scroll = terminal.process(b"\x1b[1S");
        let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;

        assert_that!(
            update.rows().iter().map(RenderRowSpan::row).collect::<Vec<_>>(),
            eq(vec![0, 1])
        );
        let _changed_rows = cached.apply_update(update)?;
        assert_that!(cached, eq(terminal.snapshot()?));
        Ok(())
    }

    #[test]
    fn test_terminal_state_render_snapshot_when_only_cursor_moves_returns_no_rows() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);
        let _baseline = terminal.render_snapshot(TerminalSnapshotScope::Full)?;

        let _outcome = terminal.process(b"\x1b[2;2H");
        let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;

        assert_that!(update.rows().len(), eq(0));
        assert_that!(update.cursor().row, eq(1));
        assert_that!(update.cursor().col, eq(1));
        Ok(())
    }

    #[test]
    fn test_terminal_state_render_snapshot_when_viewport_moves_returns_all_rows() -> rootcause::Result<()> {
        let size = terminal_size()?;
        let mut terminal = self::terminal_state(&size);
        let _outcome = terminal.process(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
        let _baseline = terminal.render_snapshot(TerminalSnapshotScope::Full)?;

        assert_that!(
            terminal.scroll_one_line(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Moved)
        );
        let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;

        assert_that!(update.rows().len(), eq(usize::from(size.rows())));
        Ok(())
    }

    #[test]
    fn test_terminal_state_render_snapshot_when_scrolled_history_is_evicted_returns_all_rows() -> rootcause::Result<()>
    {
        let size = TerminalSize::new(8, 3)?;
        let mut scrollback = MuxrConfig::default().scrollback;
        scrollback.rows = 2;
        let mut terminal = TerminalState::with_scrollback(&size, scrollback);
        let _initial = terminal.process(b"\x1b[1;1HA\x1b[2;1HB\x1b[3;1Hfixed\x1b[1;2r\x1b[1S");
        let _second = terminal.process(b"\x1b[2;1HC\x1b[1S");
        assert_that!(
            terminal.scroll_one_line(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Moved)
        );
        assert_that!(
            terminal.scroll_one_line(PaneScrollDirection::Up),
            eq(TerminalScrollMove::Moved)
        );
        let mut cached = terminal.render_snapshot(TerminalSnapshotScope::Full)?;

        let _eviction = terminal.process(b"\x1b[2;1HD\x1b[1S");
        let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;
        assert_that!(update.rows().len(), eq(usize::from(size.rows())));
        let _changed_rows = cached.apply_update(update)?;

        assert_that!(cached, eq(terminal.snapshot()?));
        Ok(())
    }

    #[test]
    fn test_terminal_state_render_snapshot_when_resized_returns_all_rows() -> rootcause::Result<()> {
        let mut terminal = self::terminal_state(&terminal_size()?);
        let _baseline = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
        let resized = TerminalSize::new(10, 5)?;

        terminal.resize(&resized);
        let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;

        assert_that!(update.rows().len(), eq(usize::from(resized.rows())));
        assert_that!(update.size(), eq(&resized));
        Ok(())
    }

    fn terminal_size() -> rootcause::Result<TerminalSize> {
        TerminalSize::new(8, 4)
    }

    fn terminal_state(size: &TerminalSize) -> TerminalState {
        TerminalState::with_scrollback(size, MuxrConfig::default().scrollback)
    }

    fn test_scrollback_dump(terminal: &mut TerminalState, style: ScrollbackDumpStyle) -> Vec<u8> {
        terminal.scrollback_dump(style)
    }

    fn snapshot_text(snapshot: &TerminalSnapshot) -> String {
        snapshot
            .rows()
            .iter()
            .flat_map(|row| row.cells().iter().map(RenderCell::text))
            .collect()
    }
}
