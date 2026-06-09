use std::collections::VecDeque;

use muxr_core::PaneScrollDirection;
use muxr_core::RenderCell;
use muxr_core::TerminalSize;
use vte::Params;
use vte::Perform;

// Match the local Zellij scroll buffer so long interactive sessions are not truncated sooner in muxr.
pub const SCROLLBACK_ROWS: usize = 50_000;

pub struct TerminalPartialScrollback {
    parser: vte::Parser,
    rows: VecDeque<Vec<RenderCell>>,
    scroll_region: TerminalScrollRegion,
    size: TerminalSize,
    viewport_offset: usize,
}

impl TerminalPartialScrollback {
    pub fn new(size: &TerminalSize) -> Self {
        Self {
            parser: vte::Parser::new(),
            rows: VecDeque::new(),
            scroll_region: TerminalScrollRegion::full(size),
            size: size.clone(),
            viewport_offset: 0,
        }
    }

    pub fn observe_byte(&mut self, byte: u8) -> Option<TerminalPreParserAction> {
        let mut performer = TerminalPartialScrollbackParser {
            action: None,
            scroll_region: self.scroll_region,
            size: &self.size,
        };
        self.parser.advance(&mut performer, &[byte]);
        self.scroll_region = performer.scroll_region;
        performer.action
    }

    pub fn push_rows(&mut self, rows: impl IntoIterator<Item = Vec<RenderCell>>) {
        for row in rows {
            self.rows.push_back(row);
            while self.rows.len() > SCROLLBACK_ROWS {
                self.rows.pop_front();
                self.viewport_offset = self.viewport_offset.saturating_sub(1);
            }
        }
    }

    pub fn captured_len(&self) -> usize {
        self.rows.len()
    }

    pub const fn captured_rows(&self) -> &VecDeque<Vec<RenderCell>> {
        &self.rows
    }

    pub const fn captured_rows_for_linefeed_at(&self, cursor_row: u16) -> Option<TerminalScrolledRows> {
        self.scroll_region.captured_rows_for_linefeed_at(cursor_row, &self.size)
    }

    pub const fn should_capture_linefeed(&self, alternate_screen: bool) -> bool {
        self.scroll_region.should_capture_linefeed(&self.size, alternate_screen)
    }

    pub const fn viewport_offset(&self) -> usize {
        self.viewport_offset
    }

    pub fn resize(&mut self, size: &TerminalSize) {
        self.size = size.clone();
        self.scroll_region = self.scroll_region.clamped_to(size);
    }

    pub fn scroll_to(&mut self, offset: usize, base_scrollback_len: usize) {
        self.viewport_offset = offset.min(self.rows.len().saturating_add(base_scrollback_len));
    }

    pub fn scroll_by(&mut self, direction: PaneScrollDirection, lines: usize, base_scrollback_len: usize) -> bool {
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
            'M' => {
                if let Some(count) = self.scroll_region.captured_rows_for_delete_lines(params) {
                    self.action = Some(TerminalPreParserAction::CaptureDeletedTopRows { count });
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
pub enum TerminalPreParserAction {
    CaptureDeletedTopRows { count: usize },
    CaptureTopRows { count: usize, full_height: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalScrolledRows {
    pub count: usize,
    pub full_height: bool,
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

    fn captured_rows_for_delete_lines(self, params: &Params) -> Option<usize> {
        if self.top != 0 {
            return None;
        }
        let count = self::primary_csi_params(params)
            .first()
            .copied()
            .filter(|value| *value != 0)
            .unwrap_or(1);
        let region_rows = self.bottom.saturating_sub(self.top).saturating_add(1);
        Some(usize::from(count.min(region_rows)))
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
        if alternate_screen || self.top != 0 {
            return false;
        }
        self.bottom < size.rows().saturating_sub(1)
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

fn primary_csi_params(params: &Params) -> Vec<u16> {
    params.iter().map(|param| param.first().copied().unwrap_or(0)).collect()
}
