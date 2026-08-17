use std::sync::Arc;

use parking_lot::Mutex;
use rio_vt::ansi::CursorShape;
use rio_vt::crosswords::Crosswords;
use rio_vt::crosswords::CrosswordsSize;
use rio_vt::crosswords::Mode;
use rio_vt::crosswords::grid::Grid;
use rio_vt::crosswords::square::Square;
use rio_vt::event::EventListener;
use rio_vt::event::RioEvent;
use rio_vt::event::WindowId;
use rio_vt::performer::handler::Processor;

pub(super) use self::input_filter::RioInputFilter;
use super::control::AlternateScreenControl;

mod input_filter;

#[derive(Default)]
pub(super) struct Events {
    pub(super) cursor_change: CursorChange,
    pub(super) replies: Vec<Vec<u8>>,
    pub(super) titles: Vec<Option<String>>,
}

impl Events {
    fn append(&mut self, mut other: Self) {
        if other.cursor_change == CursorChange::Changed {
            self.cursor_change = CursorChange::Changed;
        }
        self.replies.append(&mut other.replies);
        self.titles.append(&mut other.titles);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum CursorChange {
    #[default]
    Unchanged,
    Changed,
}

#[derive(Clone, Default)]
pub(super) struct Listener {
    events: Arc<Mutex<Events>>,
}

impl Listener {
    fn take_events(&self) -> Events {
        std::mem::take(&mut *self.events.lock())
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: RioEvent, _id: WindowId) {
        let mut events = self.events.lock();
        match event {
            RioEvent::PtyWrite(_, reply) => events.replies.push(reply.into_bytes()),
            RioEvent::ResetTitle => events.titles.push(None),
            RioEvent::Title(title) | RioEvent::TitleWithSubtitle(title, _) => {
                let title = title.trim().to_owned();
                events.titles.push((!title.is_empty()).then_some(title));
            }
            RioEvent::CursorBlinkingChange | RioEvent::CursorBlinkingChangeOnRoute(_) => {
                events.cursor_change = CursorChange::Changed;
            }
            RioEvent::PrepareRender(_)
            | RioEvent::PrepareRenderOnRoute(..)
            | RioEvent::PrepareUpdateConfig
            | RioEvent::Render
            | RioEvent::RenderRoute(_)
            | RioEvent::TerminalDamaged(_)
            | RioEvent::UpdateGraphics { .. }
            | RioEvent::GlyphProtocolQuery { .. }
            | RioEvent::Paste
            | RioEvent::Copy(_)
            | RioEvent::UpdateFontSize(_)
            | RioEvent::Scroll(_)
            | RioEvent::ToggleFullScreen
            | RioEvent::ToggleAppearanceTheme
            | RioEvent::Minimize(_)
            | RioEvent::Hide
            | RioEvent::HideOtherApplications
            | RioEvent::UpdateConfig
            | RioEvent::CreateWindow
            | RioEvent::ToggleQuake
            | RioEvent::CloseWindow
            | RioEvent::CreateNativeTab(_)
            | RioEvent::CreateConfigEditor
            | RioEvent::SelectNativeTabByIndex(_)
            | RioEvent::SelectNativeTabLast
            | RioEvent::SelectNativeTabNext
            | RioEvent::SelectNativeTabPrev
            | RioEvent::ReportToAssistant(_)
            | RioEvent::MouseCursorDirty
            | RioEvent::ClipboardStore(..)
            | RioEvent::ClipboardLoad(..)
            | RioEvent::ColorRequest(..)
            | RioEvent::TextAreaSizeRequest(..)
            | RioEvent::ProgressReport(_)
            | RioEvent::Bell
            | RioEvent::DesktopNotification { .. }
            | RioEvent::Exit
            | RioEvent::Quit
            | RioEvent::CloseTerminal(_)
            | RioEvent::ChildExited(..)
            | RioEvent::BlinkCursor(..)
            | RioEvent::SelectionScrollTick
            | RioEvent::UpdateTitles
            | RioEvent::ColorChange(..)
            | RioEvent::Noop => {}
        }
    }
}

pub(super) struct RioTerminal {
    legacy_alternate_grid: Option<Grid<Square>>,
    listener: Listener,
    // rio-vt eagerly reserves a 2 MiB synchronized-update buffer for every Processor. Muxr strips
    // modes 2026 and 2027, so this is an accepted per-pane cost until upstream offers lazy allocation.
    parser: Processor,
    terminal: Crosswords<Listener>,
}

impl RioTerminal {
    pub(super) fn new(columns: usize, rows: usize, scrollback_rows: usize) -> Self {
        let listener = Listener::default();
        let mut terminal = Crosswords::new(
            CrosswordsSize::new(columns, rows),
            CursorShape::Block,
            listener.clone(),
            WindowId::from(0),
            0,
            scrollback_rows,
        );
        // Muxr emits ANSI text to Alacritty, whose terminal grid measures each codepoint with
        // `unicode-width` and stores wide characters as two cells. Rio's mode-2027 grapheme
        // clustering would instead collapse sequences such as `👩‍🌾` into one two-cell
        // grapheme. That makes Rio's logical cursor disagree with Alacritty's cursor after the
        // raw sequence is printed. Keep the original Unicode text, but use Alacritty's
        // codepoint-cell contract so pane borders, cursor positions, and redraws stay aligned.
        terminal.set_grapheme_clustering(false);
        terminal.reset_damage();
        Self {
            legacy_alternate_grid: None,
            listener,
            parser: Processor::default(),
            terminal,
        }
    }

    pub(super) fn advance(&mut self, bytes: &[u8]) -> Events {
        let alternate_screen_before = self.terminal.mode().contains(Mode::ALT_SCREEN);
        self.parser.advance(&mut self.terminal, bytes);
        if self.terminal.mode().contains(Mode::ALT_SCREEN) != alternate_screen_before {
            self.legacy_alternate_grid = None;
        }
        self.listener.take_events()
    }

    pub(super) fn advance_with_alternate_screen_controls(
        &mut self,
        bytes: &[u8],
        controls: &[(usize, AlternateScreenControl)],
    ) -> Events {
        let mut events = Events::default();
        let mut remaining = bytes;
        let mut previous_end = 0_usize;
        for (end, control) in controls {
            let Some(chunk_len) = end.checked_sub(previous_end) else {
                events.append(self.advance(remaining));
                return events;
            };
            let Some(chunk) = remaining.get(..chunk_len) else {
                events.append(self.advance(remaining));
                return events;
            };
            events.append(self.advance(chunk));
            self.apply_alternate_screen_control(*control);
            let Some(rest) = remaining.get(chunk_len..) else {
                return events;
            };
            remaining = rest;
            previous_end = *end;
        }
        events.append(self.advance(remaining));
        events
    }

    pub(super) fn with_primary_screen<T>(&mut self, operation: impl FnOnce(&Crosswords<Listener>) -> T) -> T {
        if !self.terminal.mode().contains(Mode::ALT_SCREEN) {
            return operation(&self.terminal);
        }

        let alternate_grid = self.terminal.grid.clone();
        self.terminal.swap_alt();
        let result = operation(&self.terminal);
        self.terminal.swap_alt();
        self.terminal.grid = alternate_grid;
        result
    }

    pub(super) fn resize(&mut self, columns: usize, rows: usize) {
        self.terminal.resize(CrosswordsSize::new(columns, rows));
        if let Some(grid) = self.legacy_alternate_grid.as_mut() {
            grid.resize(false, rows, columns);
        }
    }

    pub(super) const fn terminal(&self) -> &Crosswords<Listener> {
        &self.terminal
    }

    pub(super) const fn terminal_mut(&mut self) -> &mut Crosswords<Listener> {
        &mut self.terminal
    }

    fn apply_alternate_screen_control(&mut self, control: AlternateScreenControl) {
        match (control, self.terminal.mode().contains(Mode::ALT_SCREEN)) {
            (AlternateScreenControl::EnterLegacy, false) => {
                let alternate_grid = self.legacy_alternate_grid.take();
                self.terminal.swap_alt();
                if let Some(alternate_grid) = alternate_grid {
                    self.terminal.grid = alternate_grid;
                }
            }
            (AlternateScreenControl::ExitLegacy, true) => {
                self.legacy_alternate_grid = Some(self.terminal.grid.clone());
                self.terminal.swap_alt();
            }
            (AlternateScreenControl::InvalidatePreserved, _) => self.legacy_alternate_grid = None,
            (AlternateScreenControl::EnterLegacy, true) | (AlternateScreenControl::ExitLegacy, false) => {}
        }
    }
}
