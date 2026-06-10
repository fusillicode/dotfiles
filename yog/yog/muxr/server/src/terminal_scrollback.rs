use std::collections::VecDeque;
use std::mem;
use std::num::NonZeroU16;

use muxr_core::PaneScrollDirection;
use muxr_core::RenderCell;
use muxr_core::TerminalSize;
use unicode_width::UnicodeWidthChar as _;
use vte::Params;
use vte::Perform;

pub struct TerminalScrollback {
    max_rows: usize,
    // vt100 keeps normal and alternate grids separate. muxr mirrors only the scroll-region state it needs before vt100
    // consumes a scroll byte; sharing one region lets alternate-screen TUIs poison later normal scrollback capture.
    active_screen: TerminalScreen,
    alternate_scroll_region: TerminalScrollRegion,
    normal_scroll_region: TerminalScrollRegion,
    parser: vte::Parser,
    rows: VecDeque<TerminalScrollbackRow>,
    size: TerminalSize,
    viewport_offset: usize,
}

impl TerminalScrollback {
    pub fn new(size: &TerminalSize, max_rows: usize) -> Self {
        Self {
            max_rows,
            active_screen: TerminalScreen::Normal,
            alternate_scroll_region: TerminalScrollRegion::full(size),
            normal_scroll_region: TerminalScrollRegion::full(size),
            parser: vte::Parser::new(),
            rows: VecDeque::new(),
            size: size.clone(),
            viewport_offset: 0,
        }
    }

    pub fn observe_byte(&mut self, byte: u8) -> Option<TerminalPreParserAction> {
        let mut performer = TerminalScrollbackParser {
            action: None,
            active_screen: self.active_screen,
            alternate_scroll_region: self.alternate_scroll_region,
            normal_scroll_region: self.normal_scroll_region,
            size: &self.size,
        };
        self.parser.advance(&mut performer, &[byte]);
        self.active_screen = performer.active_screen;
        self.alternate_scroll_region = performer.alternate_scroll_region;
        self.normal_scroll_region = performer.normal_scroll_region;
        performer.action
    }

    pub fn push_rows(&mut self, rows: impl IntoIterator<Item = Vec<RenderCell>>) {
        for row in rows {
            self.rows.push_back(TerminalScrollbackRow::from_cells(row));
            while self.rows.len() > self.max_rows {
                self.rows.pop_front();
                self.viewport_offset = self.viewport_offset.saturating_sub(1);
            }
        }
    }

    pub fn captured_len(&self) -> usize {
        self.rows.len()
    }

    pub fn captured_row_cells_for_view(&self, offset: usize, height: usize) -> Vec<Vec<RenderCell>> {
        let start = self.rows.len().saturating_sub(offset);
        self.rows
            .iter()
            .skip(start)
            .take(height)
            .map(TerminalScrollbackRow::cells)
            .collect()
    }

    pub fn captured_oldest_row_cells_iter(&self, count: usize) -> impl Iterator<Item = Vec<RenderCell>> + '_ {
        self.rows.iter().take(count).map(TerminalScrollbackRow::cells)
    }

    pub const fn captured_rows_for_linefeed_at(&self, cursor_row: u16) -> Option<usize> {
        self.normal_scroll_region.captured_rows_for_linefeed_at(cursor_row)
    }

    pub const fn captured_rows_for_autowrap_at(
        &self,
        cursor_row: u16,
        cursor_col: u16,
        printable_width: u16,
    ) -> Option<usize> {
        self.normal_scroll_region
            .captured_rows_for_autowrap_at(cursor_row, cursor_col, printable_width, &self.size)
    }

    pub fn autowrap_capture_possible_after_prints(
        &self,
        cursor_row: u16,
        cursor_col: u16,
        pending_print_width: usize,
        next_print_width: u16,
    ) -> bool {
        if self.active_screen != TerminalScreen::Normal {
            return false;
        }
        self.normal_scroll_region.autowrap_capture_possible_after_prints(
            cursor_row,
            cursor_col,
            pending_print_width,
            next_print_width,
            &self.size,
        )
    }

    pub fn should_capture_linefeed(&self, alternate_screen: bool) -> bool {
        !alternate_screen && self.active_screen == TerminalScreen::Normal && self.normal_scroll_region.should_capture()
    }

    pub const fn viewport_offset(&self) -> usize {
        self.viewport_offset
    }

    pub fn resize(&mut self, size: &TerminalSize) {
        self.size = size.clone();
        self.alternate_scroll_region = self.alternate_scroll_region.clamped_to(size);
        self.normal_scroll_region = self.normal_scroll_region.clamped_to(size);
    }

    pub fn scroll_to(&mut self, offset: usize) {
        self.viewport_offset = offset.min(self.rows.len());
    }

    pub fn scroll_by(&mut self, direction: PaneScrollDirection, lines: usize) -> bool {
        let before = self.viewport_offset;
        self.viewport_offset = match direction {
            PaneScrollDirection::Down => self.viewport_offset.saturating_sub(lines),
            PaneScrollDirection::Up => self.viewport_offset.saturating_add(lines).min(self.rows.len()),
        };
        self.viewport_offset != before
    }
}

// Terminal scrollback can reach tens of thousands of mostly padded rows. Store rows as runs only when that is smaller
// than raw cells; dense output must not pay extra per-cell run overhead.
enum TerminalScrollbackRow {
    Raw(Vec<RenderCell>),
    Runs(Vec<TerminalScrollbackRun>),
}

impl TerminalScrollbackRow {
    fn from_cells(cells: Vec<RenderCell>) -> Self {
        let mut run_count = 0_usize;
        let mut previous = None::<&RenderCell>;
        let mut previous_len = 0_u16;
        for cell in &cells {
            if previous.is_some_and(|previous| previous == cell) && previous_len < u16::MAX {
                previous_len = previous_len.saturating_add(1);
            } else {
                run_count = run_count.saturating_add(1);
                previous = Some(cell);
                previous_len = 1;
            }
        }

        let raw_bytes = cells.len().saturating_mul(mem::size_of::<RenderCell>());
        let run_bytes = run_count.saturating_mul(mem::size_of::<TerminalScrollbackRun>());
        if run_bytes >= raw_bytes {
            return Self::Raw(cells);
        }

        let mut runs = Vec::<TerminalScrollbackRun>::with_capacity(run_count);
        for cell in cells {
            if let Some(run) = runs.last_mut()
                && run.cell == cell
                && run.push_cell()
            {
                continue;
            }
            runs.push(TerminalScrollbackRun::new(cell));
        }
        Self::Runs(runs)
    }

    fn cells(&self) -> Vec<RenderCell> {
        match self {
            Self::Raw(cells) => cells.clone(),
            Self::Runs(runs) => {
                let len = runs
                    .iter()
                    .map(TerminalScrollbackRun::len)
                    .fold(0_usize, usize::saturating_add);
                let mut cells = Vec::with_capacity(len);
                for run in runs {
                    cells.extend(std::iter::repeat_n(run.cell.clone(), run.len()));
                }
                cells
            }
        }
    }
}

struct TerminalScrollbackRun {
    cell: RenderCell,
    len: NonZeroU16,
}

impl TerminalScrollbackRun {
    const fn new(cell: RenderCell) -> Self {
        Self {
            cell,
            len: NonZeroU16::MIN,
        }
    }

    fn len(&self) -> usize {
        usize::from(self.len.get())
    }

    fn push_cell(&mut self) -> bool {
        let Some(len) = self.len.get().checked_add(1).and_then(NonZeroU16::new) else {
            return false;
        };
        self.len = len;
        true
    }
}

struct TerminalScrollbackParser<'a> {
    action: Option<TerminalPreParserAction>,
    active_screen: TerminalScreen,
    alternate_scroll_region: TerminalScrollRegion,
    normal_scroll_region: TerminalScrollRegion,
    size: &'a TerminalSize,
}

impl Perform for TerminalScrollbackParser<'_> {
    fn print(&mut self, c: char) {
        let Some(width) = self::printable_width(c) else {
            return;
        };
        self.action = Some(TerminalPreParserAction::Printable { width });
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\x08' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r' => {
                self.action = Some(TerminalPreParserAction::SyncParser);
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if ignore || !intermediates.is_empty() {
            return;
        }

        if byte == b'c' {
            self.active_screen = TerminalScreen::Normal;
            self.alternate_scroll_region = TerminalScrollRegion::full(self.size);
            self.normal_scroll_region = TerminalScrollRegion::full(self.size);
        }
        if matches!(byte, b'7' | b'8' | b'M' | b'c') {
            self.action = Some(TerminalPreParserAction::SyncParser);
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }

        match (intermediates, action) {
            ([], 'S') => {
                if let Some(count) = self.active_scroll_region().captured_rows_for_scroll_up(params) {
                    self.action = Some(TerminalPreParserAction::CaptureTopRows { count });
                } else {
                    self.action = Some(TerminalPreParserAction::SyncParser);
                }
            }
            ([], 'M') => {
                if let Some(count) = self.active_scroll_region().captured_rows_for_delete_lines(params) {
                    self.action = Some(TerminalPreParserAction::CaptureDeletedTopRows { count });
                } else {
                    self.action = Some(TerminalPreParserAction::SyncParser);
                }
            }
            ([], 'r') => {
                self.set_active_scroll_region(TerminalScrollRegion::from_decstbm(params, self.size));
                self.action = Some(TerminalPreParserAction::SyncParser);
            }
            ([b'?'], 'h') => {
                self.apply_private_mode(params, true);
                self.action = Some(TerminalPreParserAction::SyncParser);
            }
            ([b'?'], 'l') => {
                self.apply_private_mode(params, false);
                self.action = Some(TerminalPreParserAction::SyncParser);
            }
            _ => {
                self.action = Some(TerminalPreParserAction::SyncParser);
            }
        }
    }
}

impl TerminalScrollbackParser<'_> {
    const fn active_scroll_region(&self) -> TerminalScrollRegion {
        match self.active_screen {
            TerminalScreen::Alternate => self.alternate_scroll_region,
            TerminalScreen::Normal => self.normal_scroll_region,
        }
    }

    fn apply_private_mode(&mut self, params: &Params, enabled: bool) {
        for param in self::primary_csi_params(params) {
            match (enabled, param) {
                (true, 47) => self.active_screen = TerminalScreen::Alternate,
                (true, 1049) => {
                    self.active_screen = TerminalScreen::Alternate;
                    self.alternate_scroll_region = TerminalScrollRegion::full(self.size);
                }
                (false, 47 | 1049) => self.active_screen = TerminalScreen::Normal,
                _ => {}
            }
        }
    }

    const fn set_active_scroll_region(&mut self, region: TerminalScrollRegion) {
        match self.active_screen {
            TerminalScreen::Alternate => self.alternate_scroll_region = region,
            TerminalScreen::Normal => self.normal_scroll_region = region,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalPreParserAction {
    CaptureDeletedTopRows {
        count: usize,
    },
    CaptureTopRows {
        count: usize,
    },
    Printable {
        width: u16,
    },
    /// Flush bytes through the current byte because the terminal state changed without moving rows into muxr history.
    SyncParser,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalScreen {
    Alternate,
    Normal,
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

    fn captured_rows_for_scroll_up(self, params: &Params) -> Option<usize> {
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

    const fn captured_rows_for_linefeed_at(self, cursor_row: u16) -> Option<usize> {
        if self.top != 0 || cursor_row != self.bottom {
            return None;
        }
        Some(1)
    }

    const fn captured_rows_for_autowrap_at(
        self,
        cursor_row: u16,
        cursor_col: u16,
        printable_width: u16,
        size: &TerminalSize,
    ) -> Option<usize> {
        if self.top != 0 || cursor_row != self.bottom {
            return None;
        }
        if cursor_col <= size.cols().saturating_sub(printable_width) {
            return None;
        }
        Some(1)
    }

    fn autowrap_capture_possible_after_prints(
        self,
        cursor_row: u16,
        cursor_col: u16,
        pending_print_width: usize,
        next_print_width: u16,
        size: &TerminalSize,
    ) -> bool {
        if self.top != 0 || cursor_row > self.bottom {
            return false;
        }
        let rows_to_bottom = usize::from(self.bottom.saturating_sub(cursor_row));
        let cols = usize::from(size.cols());
        // Keep the batching guard aligned with vt100: a printable scrolls only when the current column is greater than
        // `cols - width`. If this says "maybe", TerminalState flushes pending bytes and asks vt100 for the exact
        // cursor.
        let next_scroll_col = usize::from(size.cols().saturating_sub(next_print_width)).saturating_add(1);
        let cells_until_scroll_candidate = rows_to_bottom
            .saturating_mul(cols)
            .saturating_add(next_scroll_col.saturating_sub(usize::from(cursor_col)));
        pending_print_width >= cells_until_scroll_candidate
    }

    const fn should_capture(self) -> bool {
        self.top == 0
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

fn printable_width(c: char) -> Option<u16> {
    if c == '\u{fffd}' || ('\u{80}'..'\u{a0}').contains(&c) {
        return None;
    }
    let width = c.width();
    if width.is_none() && u32::from(c) < 256 {
        return None;
    }
    Some(u16::try_from(width.unwrap_or(1)).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use muxr_core::RenderStyle;

    use super::*;

    #[test]
    fn test_terminal_scrollback_push_rows_when_row_is_padded_stores_compact_runs() -> rootcause::Result<()> {
        let mut scrollback = TerminalScrollback::new(&TerminalSize::new(120, 4)?, 50_000);
        let mut row = vec![RenderCell::narrow("codex", RenderStyle::default())];
        row.extend(std::iter::repeat_n(
            RenderCell::narrow(" ", RenderStyle::default()),
            119,
        ));

        scrollback.push_rows([row.clone()]);

        assert2::assert!(let Some(TerminalScrollbackRow::Runs(stored_runs)) = scrollback.rows.front());
        assert2::assert!(stored_runs.len() < row.len() / 2);
        pretty_assertions::assert_eq!(scrollback.captured_row_cells_for_view(1, 1), vec![row]);
        Ok(())
    }

    #[test]
    fn test_terminal_scrollback_push_rows_when_row_is_dense_stores_raw_cells() -> rootcause::Result<()> {
        let mut scrollback = TerminalScrollback::new(&TerminalSize::new(120, 4)?, 50_000);
        let row = (0..120)
            .map(|col| RenderCell::narrow(format!("{col:03}"), RenderStyle::default()))
            .collect::<Vec<_>>();

        scrollback.push_rows([row.clone()]);

        assert2::assert!(let Some(TerminalScrollbackRow::Raw(stored_cells)) = scrollback.rows.front());
        pretty_assertions::assert_eq!(stored_cells, &row);
        pretty_assertions::assert_eq!(scrollback.captured_row_cells_for_view(1, 1), vec![row]);
        Ok(())
    }
}
