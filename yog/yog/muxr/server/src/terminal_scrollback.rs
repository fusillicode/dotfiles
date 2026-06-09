use std::collections::VecDeque;
use std::mem;
use std::num::NonZeroU16;

use muxr_core::PaneScrollDirection;
use muxr_core::RenderCell;
use muxr_core::TerminalSize;
use vte::Params;
use vte::Perform;

pub struct TerminalPartialScrollback {
    max_rows: usize,
    parser: vte::Parser,
    rows: VecDeque<TerminalScrollbackRow>,
    scroll_region: TerminalScrollRegion,
    size: TerminalSize,
    viewport_offset: usize,
}

impl TerminalPartialScrollback {
    pub fn new(size: &TerminalSize, max_rows: usize) -> Self {
        Self {
            max_rows,
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

    pub fn captured_oldest_row_cells(&self, count: usize) -> Vec<Vec<RenderCell>> {
        self.rows.iter().take(count).map(TerminalScrollbackRow::cells).collect()
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

// Partial-scroll-region history can reach tens of thousands of mostly padded rows. Store rows as runs only when that is
// smaller than raw cells; dense output must not pay extra per-cell run overhead.
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

#[cfg(test)]
mod tests {
    use muxr_core::RenderStyle;

    use super::*;

    #[test]
    fn test_terminal_partial_scrollback_push_rows_when_row_is_padded_stores_compact_runs() -> rootcause::Result<()> {
        let mut scrollback = TerminalPartialScrollback::new(&TerminalSize::new(120, 4)?, 50_000);
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
    fn test_terminal_partial_scrollback_push_rows_when_row_is_dense_stores_raw_cells() -> rootcause::Result<()> {
        let mut scrollback = TerminalPartialScrollback::new(&TerminalSize::new(120, 4)?, 50_000);
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
