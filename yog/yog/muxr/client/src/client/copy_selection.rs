use std::collections::BTreeMap;

use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::RenderCellWidth;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::render::FrameBuffer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionInput {
    Start(ClientMousePosition),
    Update(ClientMousePosition),
    End(ClientMousePosition),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SelectionState {
    // Copy text is cached by stable content row so viewport scrolling moves the highlight without dropping rows that
    // have already passed off-screen during a drag selection. The final joined string is built lazily on copy so drag
    // updates do not rebuild the full selected text every mouse packet.
    cached_rows: BTreeMap<u64, Vec<CachedSelectionCell>>,
    drag: Option<SelectionDrag>,
    selected: Option<SelectionRange>,
}

impl SelectionState {
    pub fn apply(
        &mut self,
        input: SelectionInput,
        regions: &PaneRegionsSnapshot,
        frame_buffer: &FrameBuffer,
    ) -> rootcause::Result<bool> {
        let previous = self.selected.clone();
        match input {
            SelectionInput::Start(position) => self.start(position, regions, frame_buffer),
            SelectionInput::Update(position) => self.update(position, frame_buffer)?,
            SelectionInput::End(position) => self.end(position, frame_buffer)?,
        }
        Ok(self.selected != previous)
    }

    pub fn clear_if_regions_changed(&mut self, regions: &PaneRegionsSnapshot) -> bool {
        let previous = self.selected.clone();
        self.drag = self.drag.take().and_then(|drag| {
            self::matching_region(regions, drag.region.id()).map(|region| SelectionDrag {
                anchor: drag.anchor,
                raw_anchor: self::clamp_to_region(drag.raw_anchor, &region),
                region,
            })
        });
        self.selected = self.selected.take().and_then(|selected| {
            self::matching_region(regions, selected.region.id()).map(|region| selected.with_region(region))
        });
        if self.selected.is_none() {
            self.cached_rows.clear();
        } else {
            self.retain_cached_rows();
        }
        self.selected != previous
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selected
            .as_ref()
            .and_then(|selection| self::selected_text(&self.cached_rows, selection))
            .filter(|text| !text.is_empty())
    }

    pub fn select_word(
        &mut self,
        position: ClientMousePosition,
        regions: &PaneRegionsSnapshot,
        frame_buffer: &FrameBuffer,
    ) -> rootcause::Result<bool> {
        let previous = self.selected.clone();
        self.drag = None;
        self.set_selected(self::word_selection_at(position, regions, frame_buffer), frame_buffer)?;
        Ok(self.selected != previous)
    }

    #[must_use]
    pub const fn range(&self) -> Option<&SelectionRange> {
        self.selected.as_ref()
    }

    #[must_use]
    pub fn drag_region(&self) -> Option<&PaneRegionSnapshot> {
        self.drag.as_ref().map(|drag| &drag.region)
    }

    pub fn refresh_visible_rows(&mut self, frame_buffer: &FrameBuffer) -> rootcause::Result<()> {
        self.rebuild_selected_text(frame_buffer)
    }

    fn start(&mut self, position: ClientMousePosition, regions: &PaneRegionsSnapshot, frame_buffer: &FrameBuffer) {
        let Some(region) = regions.pane_at(position) else {
            self.clear();
            return;
        };
        let region = region.clone();
        let raw_anchor = self::clamp_to_region(position, &region);
        let Some(anchor) =
            self::content_position(self::selectable_position(raw_anchor, &region, frame_buffer), &region)
        else {
            self.clear();
            return;
        };
        self.drag = Some(SelectionDrag {
            anchor,
            raw_anchor,
            region,
        });
        self.selected = None;
        self.cached_rows.clear();
    }

    fn update(&mut self, position: ClientMousePosition, frame_buffer: &FrameBuffer) -> rootcause::Result<()> {
        let Some(drag) = &self.drag else {
            return Ok(());
        };
        let raw_focus = self::clamp_to_region(position, &drag.region);
        let Some(focus) = self::content_position(
            self::selectable_position(raw_focus, &drag.region, frame_buffer),
            &drag.region,
        ) else {
            self.set_selected(None, frame_buffer)?;
            return Ok(());
        };
        self.set_selected(
            Some(SelectionRange {
                anchor: drag.anchor,
                focus,
                region: drag.region.clone(),
            }),
            frame_buffer,
        )
    }

    fn end(&mut self, position: ClientMousePosition, frame_buffer: &FrameBuffer) -> rootcause::Result<()> {
        let Some(drag) = self.drag.take() else {
            return Ok(());
        };
        let raw_focus = self::clamp_to_region(position, &drag.region);
        let Some(focus) = self::content_position(
            self::selectable_position(raw_focus, &drag.region, frame_buffer),
            &drag.region,
        ) else {
            self.set_selected(None, frame_buffer)?;
            return Ok(());
        };
        let selected = (raw_focus != drag.raw_anchor).then_some(SelectionRange {
            anchor: drag.anchor,
            focus,
            region: drag.region,
        });
        self.set_selected(selected, frame_buffer)
    }

    fn set_selected(&mut self, selected: Option<SelectionRange>, frame_buffer: &FrameBuffer) -> rootcause::Result<()> {
        self.selected = selected;
        self.rebuild_selected_text(frame_buffer)?;
        Ok(())
    }

    fn rebuild_selected_text(&mut self, frame_buffer: &FrameBuffer) -> rootcause::Result<()> {
        if self.selected.is_none() {
            self.cached_rows.clear();
            return Ok(());
        }

        self.retain_cached_rows();
        if let Some(selection) = self.selected.as_ref() {
            self::cache_visible_selected_rows(&mut self.cached_rows, frame_buffer, selection)?;
        }
        Ok(())
    }

    fn retain_cached_rows(&mut self) {
        let Some(selected) = self.selected.as_ref() else {
            self.cached_rows.clear();
            return;
        };
        self.cached_rows
            .retain(|content_row, _| selected.contains_content_row(*content_row));
    }

    fn clear(&mut self) {
        self.cached_rows.clear();
        self.drag = None;
        self.selected = None;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectionRange {
    anchor: SelectionContentPosition,
    focus: SelectionContentPosition,
    region: PaneRegionSnapshot,
}

impl SelectionRange {
    #[must_use]
    pub fn contains(&self, row: u16, col: u16) -> bool {
        if !self.region.contains(row, col) {
            return false;
        }
        let Some(position) = self::content_position(ClientMousePosition { row, col }, &self.region) else {
            return false;
        };

        let bounds = self.bounds();
        if position.row < bounds.start.row || position.row > bounds.end.row {
            return false;
        }

        if bounds.start.row == bounds.end.row {
            return position.col >= bounds.start.col && position.col <= bounds.end.col;
        }
        if position.row == bounds.start.row {
            return position.col >= bounds.start.col;
        }
        if position.row == bounds.end.row {
            return position.col <= bounds.end.col;
        }
        true
    }

    #[must_use]
    pub fn row_bounds(&self) -> Option<(u16, u16)> {
        let bounds = self.bounds();
        let viewport_start = self.region.visible_top_row();
        let viewport_end = viewport_start.saturating_add(u64::from(self.region.rows().saturating_sub(1)));
        let start = bounds.start.row.max(viewport_start);
        let end = bounds.end.row.min(viewport_end);
        if start > end {
            return None;
        }

        Some((
            self::visible_row_for_content_row(&self.region, start)?,
            self::visible_row_for_content_row(&self.region, end)?,
        ))
    }

    #[must_use]
    pub fn bounds_positions(&self) -> Option<(ClientMousePosition, ClientMousePosition)> {
        let bounds = self.bounds();
        Some((
            self::visible_position(&self.region, bounds.start)?,
            self::visible_position(&self.region, bounds.end)?,
        ))
    }

    #[must_use]
    pub const fn pane_id(&self) -> &PaneId {
        self.region.id()
    }

    fn bounds(&self) -> SelectionBounds {
        if (self.anchor.row, self.anchor.col) <= (self.focus.row, self.focus.col) {
            SelectionBounds {
                start: self.anchor,
                end: self.focus,
            }
        } else {
            SelectionBounds {
                start: self.focus,
                end: self.anchor,
            }
        }
    }

    fn contains_content_row(&self, row: u64) -> bool {
        let bounds = self.bounds();
        row >= bounds.start.row && row <= bounds.end.row
    }

    fn with_region(self, region: PaneRegionSnapshot) -> Self {
        let last_col = region.cols().saturating_sub(1);
        Self {
            anchor: self.anchor.clamp_col(last_col),
            focus: self.focus.clamp_col(last_col),
            region,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SelectionContentPosition {
    col: u16,
    row: u64,
}

impl SelectionContentPosition {
    #[must_use]
    const fn clamp_col(self, last_col: u16) -> Self {
        Self {
            col: if self.col > last_col { last_col } else { self.col },
            row: self.row,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SelectionDrag {
    anchor: SelectionContentPosition,
    raw_anchor: ClientMousePosition,
    region: PaneRegionSnapshot,
}

#[derive(Clone, Copy)]
struct SelectionBounds {
    end: SelectionContentPosition,
    start: SelectionContentPosition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CachedSelectionCell {
    text: String,
    width: RenderCellWidth,
}

pub fn copy_to_clipboard(text: &str) -> rootcause::Result<()> {
    let mut bytes = text.as_bytes();
    Ok(ytil_sys::file::cp_to_system_clipboard(&mut bytes).context("failed to copy muxr selection to clipboard")?)
}

fn selected_text(cached_rows: &BTreeMap<u64, Vec<CachedSelectionCell>>, selection: &SelectionRange) -> Option<String> {
    let bounds = selection.bounds();
    let mut lines = Vec::new();
    for content_row in bounds.start.row..=bounds.end.row {
        let Some(cells) = cached_rows.get(&content_row) else {
            // A selected range is copyable only when every selected content row was rendered and cached; copying a
            // subset would silently drop text after skipped/coalesced edge-scroll renders.
            return None;
        };
        let start_col = if content_row == bounds.start.row {
            bounds.start.col
        } else {
            0
        };
        let end_col = if content_row == bounds.end.row {
            bounds.end.col
        } else {
            selection.region.cols().saturating_sub(1)
        };
        lines.push(self::selected_row_text(cells, start_col, end_col));
    }
    Some(lines.join("\n"))
}

fn cache_visible_selected_rows(
    cached_rows: &mut BTreeMap<u64, Vec<CachedSelectionCell>>,
    frame_buffer: &FrameBuffer,
    selection: &SelectionRange,
) -> rootcause::Result<()> {
    let bounds = selection.bounds();
    let viewport_start = selection.region.visible_top_row();
    let viewport_end = viewport_start.saturating_add(u64::from(selection.region.rows().saturating_sub(1)));
    let start = bounds.start.row.max(viewport_start);
    let end = bounds.end.row.min(viewport_end);
    if start > end {
        return Ok(());
    }

    for content_row in start..=end {
        let Some(visible_row) = self::visible_row_for_content_row(&selection.region, content_row) else {
            continue;
        };
        cached_rows.insert(
            content_row,
            self::cached_row_cells(frame_buffer, visible_row, &selection.region)?,
        );
    }
    Ok(())
}

fn cached_row_cells(
    frame_buffer: &FrameBuffer,
    row: u16,
    region: &PaneRegionSnapshot,
) -> rootcause::Result<Vec<CachedSelectionCell>> {
    let mut cells = Vec::with_capacity(usize::from(region.cols()));
    for local_col in 0..region.cols() {
        let absolute_col = self::absolute_col(region, local_col)?;
        let cell = frame_buffer.cell(row, absolute_col).map_or_else(
            || CachedSelectionCell {
                text: String::new(),
                width: RenderCellWidth::Narrow,
            },
            |cell| CachedSelectionCell {
                text: cell.text().to_owned(),
                width: cell.width(),
            },
        );
        cells.push(cell);
    }
    Ok(cells)
}

fn selected_row_text(cells: &[CachedSelectionCell], start_col: u16, end_col: u16) -> String {
    let mut line = String::new();
    for local_col in start_col..=end_col {
        let Some(cell) = cells.get(usize::from(local_col)) else {
            continue;
        };
        if matches!(cell.width, RenderCellWidth::WideContinuation) {
            continue;
        }
        if cell.text.is_empty() {
            line.push(' ');
        } else {
            line.push_str(&cell.text);
        }
    }
    while line.ends_with(' ') {
        line.pop();
    }
    line
}

pub fn word_selection_at(
    position: ClientMousePosition,
    regions: &PaneRegionsSnapshot,
    frame_buffer: &FrameBuffer,
) -> Option<SelectionRange> {
    let region = regions.pane_at(position)?.clone();
    let position = self::clamp_to_region(position, &region);
    if !self::is_word_cell(frame_buffer, position.row, position.col, &region) {
        return None;
    }

    let start_col = self::word_start_col(frame_buffer, position.row, position.col, &region);
    let end_col = self::word_end_col(frame_buffer, position.row, position.col, &region);
    Some(SelectionRange {
        anchor: self::content_position(
            ClientMousePosition {
                row: position.row,
                col: start_col,
            },
            &region,
        )?,
        focus: self::content_position(
            ClientMousePosition {
                row: position.row,
                col: end_col,
            },
            &region,
        )?,
        region: region.clone(),
    })
}

fn word_start_col(frame_buffer: &FrameBuffer, row: u16, col: u16, region: &PaneRegionSnapshot) -> u16 {
    let mut start_col = col;
    while start_col > region.col() {
        let previous_col = start_col.saturating_sub(1);
        if !self::is_word_cell(frame_buffer, row, previous_col, region) {
            break;
        }
        start_col = previous_col;
    }
    start_col
}

fn word_end_col(frame_buffer: &FrameBuffer, row: u16, col: u16, region: &PaneRegionSnapshot) -> u16 {
    let mut end_col = col;
    let last_col = self::last_region_col_saturating(region);
    while end_col < last_col {
        let next_col = end_col.saturating_add(1);
        if !self::is_word_cell(frame_buffer, row, next_col, region) {
            break;
        }
        end_col = next_col;
    }
    end_col
}

fn is_word_cell(frame_buffer: &FrameBuffer, row: u16, col: u16, region: &PaneRegionSnapshot) -> bool {
    if !region.contains(row, col) {
        return false;
    }
    let Some(cell) = frame_buffer.cell(row, col) else {
        return false;
    };

    if matches!(cell.width(), RenderCellWidth::WideContinuation) {
        let Some(previous_col) = col.checked_sub(1) else {
            return false;
        };
        if !region.contains(row, previous_col) {
            return false;
        }
        let Some(previous_cell) = frame_buffer.cell(row, previous_col) else {
            return false;
        };
        return matches!(previous_cell.width(), RenderCellWidth::Wide)
            && !self::cell_text_is_whitespace(previous_cell.text());
    }

    !self::cell_text_is_whitespace(cell.text())
}

fn cell_text_is_whitespace(text: &str) -> bool {
    text.is_empty() || text.chars().all(char::is_whitespace)
}

fn matching_region(regions: &PaneRegionsSnapshot, pane_id: &PaneId) -> Option<PaneRegionSnapshot> {
    regions.regions().iter().find(|region| region.id() == pane_id).cloned()
}

fn content_position(position: ClientMousePosition, region: &PaneRegionSnapshot) -> Option<SelectionContentPosition> {
    if !region.contains(position.row, position.col) {
        return None;
    }
    let row = region
        .visible_top_row()
        .checked_add(u64::from(position.row.saturating_sub(region.row())))?;
    Some(SelectionContentPosition {
        col: position.col.saturating_sub(region.col()),
        row,
    })
}

fn visible_position(region: &PaneRegionSnapshot, position: SelectionContentPosition) -> Option<ClientMousePosition> {
    if position.col >= region.cols() {
        return None;
    }
    let row = self::visible_row_for_content_row(region, position.row)?;
    Some(ClientMousePosition {
        row,
        col: region.col().checked_add(position.col)?,
    })
}

fn visible_row_for_content_row(region: &PaneRegionSnapshot, row: u64) -> Option<u16> {
    let local_row = row.checked_sub(region.visible_top_row())?;
    let local_row = u16::try_from(local_row).ok()?;
    if local_row >= region.rows() {
        return None;
    }
    region.row().checked_add(local_row)
}

fn absolute_col(region: &PaneRegionSnapshot, local_col: u16) -> rootcause::Result<u16> {
    region
        .col()
        .checked_add(local_col)
        .ok_or_else(|| report!("muxr pane region column range overflowed"))
}

fn selectable_position(
    position: ClientMousePosition,
    region: &PaneRegionSnapshot,
    frame_buffer: &FrameBuffer,
) -> ClientMousePosition {
    let position = self::clamp_to_region(position, region);
    let Some(cell) = frame_buffer.cell(position.row, position.col) else {
        return position;
    };
    if !matches!(cell.width(), RenderCellWidth::WideContinuation) {
        return position;
    }
    let Some(previous_col) = position.col.checked_sub(1) else {
        return position;
    };
    if !region.contains(position.row, previous_col) {
        return position;
    }
    let Some(previous_cell) = frame_buffer.cell(position.row, previous_col) else {
        return position;
    };
    // Mouse reports can land on the continuation half of a wide cell; snap back so copy/render keeps the glyph.
    if matches!(previous_cell.width(), RenderCellWidth::Wide) {
        ClientMousePosition {
            row: position.row,
            col: previous_col,
        }
    } else {
        position
    }
}

fn clamp_to_region(position: ClientMousePosition, region: &PaneRegionSnapshot) -> ClientMousePosition {
    ClientMousePosition {
        row: position
            .row
            .clamp(region.row(), self::last_region_row_saturating(region)),
        col: position
            .col
            .clamp(region.col(), self::last_region_col_saturating(region)),
    }
}

const fn last_region_col_saturating(region: &PaneRegionSnapshot) -> u16 {
    region.col().saturating_add(region.cols().saturating_sub(1))
}

const fn last_region_row_saturating(region: &PaneRegionSnapshot) -> u16 {
    region.row().saturating_add(region.rows().saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use muxr_core::RenderBaseline;
    use muxr_core::RenderCell;
    use muxr_core::RenderCursor;
    use muxr_core::RenderRowSpan;
    use muxr_core::RenderStyle;
    use muxr_core::RenderUpdate;
    use muxr_core::TerminalSize;

    use super::*;

    #[test]
    fn test_selection_state_when_drag_crosses_pane_border_clamps_to_start_pane() -> rootcause::Result<()> {
        let mut selection = SelectionState::default();
        let frame_buffer = FrameBuffer::default();

        assert2::assert!(!selection.apply(
            SelectionInput::Start(ClientMousePosition { row: 0, col: 2 }),
            &pane_regions()?,
            &frame_buffer,
        )?);
        assert2::assert!(selection.apply(
            SelectionInput::Update(ClientMousePosition { row: 0, col: 8 }),
            &pane_regions()?,
            &frame_buffer,
        )?);

        let range = selection
            .range()
            .ok_or_else(|| report!("expected muxr selection range"))?;
        assert2::assert!(range.contains(0, 4));
        assert2::assert!(!range.contains(0, 5));
        assert2::assert!(!range.contains(0, 6));
        Ok(())
    }

    #[test]
    fn test_selection_state_selected_text_when_vertical_split_exists_copies_only_selected_pane() -> rootcause::Result<()>
    {
        let mut frame_buffer = FrameBuffer::default();
        frame_buffer.apply(RenderUpdate::Baseline(render_baseline()?))?;
        let mut selection = SelectionState::default();

        selection.apply(
            SelectionInput::Start(ClientMousePosition { row: 0, col: 1 }),
            &pane_regions()?,
            &frame_buffer,
        )?;
        selection.apply(
            SelectionInput::End(ClientMousePosition { row: 0, col: 9 }),
            &pane_regions()?,
            &frame_buffer,
        )?;

        pretty_assertions::assert_eq!(selection.selected_text(), Some("eft".to_owned()));
        Ok(())
    }

    #[test]
    fn test_selection_state_selected_text_when_drag_starts_on_wide_continuation_copies_wide_cell()
    -> rootcause::Result<()> {
        let mut frame_buffer = FrameBuffer::default();
        frame_buffer.apply(RenderUpdate::Baseline(wide_render_baseline()?))?;
        let mut selection = SelectionState::default();

        selection.apply(
            SelectionInput::Start(ClientMousePosition { row: 0, col: 1 }),
            &wide_pane_regions()?,
            &frame_buffer,
        )?;
        selection.apply(
            SelectionInput::End(ClientMousePosition { row: 0, col: 2 }),
            &wide_pane_regions()?,
            &frame_buffer,
        )?;

        let range = selection
            .range()
            .ok_or_else(|| report!("expected muxr wide-cell drag selection range"))?;
        assert2::assert!(range.contains(0, 0));
        assert2::assert!(range.contains(0, 1));
        pretty_assertions::assert_eq!(selection.selected_text(), Some("表".to_owned()));
        Ok(())
    }

    #[test]
    fn test_selection_state_select_word_when_word_is_clicked_selects_whitespace_delimited_word() -> rootcause::Result<()>
    {
        let mut frame_buffer = FrameBuffer::default();
        frame_buffer.apply(RenderUpdate::Baseline(render_baseline()?))?;
        let mut selection = SelectionState::default();

        assert2::assert!(selection.select_word(
            ClientMousePosition { row: 0, col: 8 },
            &pane_regions()?,
            &frame_buffer,
        )?);

        pretty_assertions::assert_eq!(selection.selected_text(), Some("right".to_owned()));
        Ok(())
    }

    #[rstest::rstest]
    #[case::wide_start(0)]
    #[case::wide_continuation(1)]
    fn test_selection_state_select_word_when_wide_cell_is_clicked_selects_whole_wide_cell(
        #[case] col: u16,
    ) -> rootcause::Result<()> {
        let mut frame_buffer = FrameBuffer::default();
        frame_buffer.apply(RenderUpdate::Baseline(wide_render_baseline()?))?;
        let mut selection = SelectionState::default();

        assert2::assert!(selection.select_word(
            ClientMousePosition { row: 0, col },
            &wide_pane_regions()?,
            &frame_buffer,
        )?);

        let range = selection
            .range()
            .ok_or_else(|| report!("expected muxr wide-cell selection range"))?;
        assert2::assert!(range.contains(0, 0));
        assert2::assert!(range.contains(0, 1));
        pretty_assertions::assert_eq!(selection.selected_text(), Some("表".to_owned()));
        Ok(())
    }

    #[test]
    fn test_selection_state_when_pane_scrolls_keeps_text_and_moves_highlight_with_content() -> rootcause::Result<()> {
        let mut frame_buffer = FrameBuffer::default();
        frame_buffer.apply(RenderUpdate::Baseline(three_row_render_baseline("aa", "bb", "cc")?))?;
        let mut selection = SelectionState::default();

        selection.apply(
            SelectionInput::Start(ClientMousePosition { row: 1, col: 0 }),
            &three_row_pane_regions(10)?,
            &frame_buffer,
        )?;
        selection.apply(
            SelectionInput::End(ClientMousePosition { row: 1, col: 1 }),
            &three_row_pane_regions(10)?,
            &frame_buffer,
        )?;
        frame_buffer.apply(RenderUpdate::Baseline(three_row_render_baseline("zz", "aa", "bb")?))?;

        assert2::assert!(selection.clear_if_regions_changed(&three_row_pane_regions(9)?));

        let range = selection
            .range()
            .ok_or_else(|| report!("expected muxr scrolled selection range"))?;
        assert2::assert!(range.contains(2, 0));
        assert2::assert!(!range.contains(1, 0));
        pretty_assertions::assert_eq!(selection.selected_text(), Some("bb".to_owned()));
        Ok(())
    }

    #[test]
    fn test_selection_state_when_edge_drag_scrolls_keeps_offscreen_selected_text() -> rootcause::Result<()> {
        let mut frame_buffer = FrameBuffer::default();
        frame_buffer.apply(RenderUpdate::Baseline(three_row_render_baseline("aa", "bb", "cc")?))?;
        let mut selection = SelectionState::default();

        selection.apply(
            SelectionInput::Start(ClientMousePosition { row: 0, col: 0 }),
            &three_row_pane_regions(9)?,
            &frame_buffer,
        )?;
        selection.apply(
            SelectionInput::Update(ClientMousePosition { row: 2, col: 1 }),
            &three_row_pane_regions(9)?,
            &frame_buffer,
        )?;
        frame_buffer.apply(RenderUpdate::Baseline(three_row_render_baseline("bb", "cc", "dd")?))?;
        assert2::assert!(selection.clear_if_regions_changed(&three_row_pane_regions(10)?));
        selection.refresh_visible_rows(&frame_buffer)?;
        selection.apply(
            SelectionInput::Update(ClientMousePosition { row: 2, col: 1 }),
            &three_row_pane_regions(10)?,
            &frame_buffer,
        )?;

        pretty_assertions::assert_eq!(selection.selected_text(), Some("aa\nbb\ncc\ndd".to_owned()));
        Ok(())
    }

    #[test]
    fn test_selection_state_selected_text_when_cached_row_is_missing_returns_none() -> rootcause::Result<()> {
        let mut frame_buffer = FrameBuffer::default();
        frame_buffer.apply(RenderUpdate::Baseline(three_row_render_baseline("aa", "bb", "cc")?))?;
        let mut selection = SelectionState::default();

        selection.apply(
            SelectionInput::Start(ClientMousePosition { row: 0, col: 0 }),
            &three_row_pane_regions(9)?,
            &frame_buffer,
        )?;
        selection.apply(
            SelectionInput::Update(ClientMousePosition { row: 2, col: 1 }),
            &three_row_pane_regions(9)?,
            &frame_buffer,
        )?;
        frame_buffer.apply(RenderUpdate::Baseline(three_row_render_baseline("ee", "ff", "gg")?))?;
        assert2::assert!(selection.clear_if_regions_changed(&three_row_pane_regions(13)?));
        selection.refresh_visible_rows(&frame_buffer)?;
        selection.apply(
            SelectionInput::Update(ClientMousePosition { row: 2, col: 1 }),
            &three_row_pane_regions(13)?,
            &frame_buffer,
        )?;

        pretty_assertions::assert_eq!(selection.selected_text(), None);
        Ok(())
    }

    fn pane_regions() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![
            PaneRegionSnapshot::new(PaneId::new("pane-1")?, 0, 0, 5, 1, muxr_core::PaneMouseMode::None, 0)?,
            PaneRegionSnapshot::new(PaneId::new("pane-2")?, 6, 0, 5, 1, muxr_core::PaneMouseMode::None, 0)?,
        ])
    }

    fn wide_pane_regions() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![PaneRegionSnapshot::new(
            PaneId::new("pane-1")?,
            0,
            0,
            3,
            1,
            muxr_core::PaneMouseMode::None,
            0,
        )?])
    }

    fn three_row_pane_regions(visible_top_row: u64) -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![PaneRegionSnapshot::new(
            PaneId::new("pane-1")?,
            0,
            0,
            2,
            3,
            muxr_core::PaneMouseMode::None,
            visible_top_row,
        )?])
    }

    fn render_baseline() -> rootcause::Result<RenderBaseline> {
        RenderBaseline::new(
            1,
            TerminalSize::new(11, 1)?,
            RenderCursor {
                row: 0,
                col: 0,
                visible: true,
            },
            vec![RenderRowSpan::new(
                0,
                0,
                "left |right"
                    .chars()
                    .map(|ch| RenderCell::narrow(ch.to_string(), RenderStyle::default()))
                    .collect(),
            )?],
        )
    }

    fn wide_render_baseline() -> rootcause::Result<RenderBaseline> {
        let style = RenderStyle::default();
        RenderBaseline::new(
            1,
            TerminalSize::new(3, 1)?,
            RenderCursor {
                row: 0,
                col: 0,
                visible: true,
            },
            vec![RenderRowSpan::new(
                0,
                0,
                vec![
                    RenderCell::wide("表", style),
                    RenderCell::wide_continuation(style),
                    RenderCell::narrow(" ", style),
                ],
            )?],
        )
    }

    fn three_row_render_baseline(first: &str, second: &str, third: &str) -> rootcause::Result<RenderBaseline> {
        RenderBaseline::new(
            1,
            TerminalSize::new(2, 3)?,
            RenderCursor {
                row: 0,
                col: 0,
                visible: true,
            },
            vec![
                RenderRowSpan::new(0, 0, first.chars().map(render_cell).collect())?,
                RenderRowSpan::new(1, 0, second.chars().map(render_cell).collect())?,
                RenderRowSpan::new(2, 0, third.chars().map(render_cell).collect())?,
            ],
        )
    }

    fn render_cell(ch: char) -> RenderCell {
        RenderCell::narrow(ch.to_string(), RenderStyle::default())
    }
}
