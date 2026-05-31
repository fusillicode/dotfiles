use muxr_core::PaneId;

use crate::layout::PaneFocusDirection;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaneLayout {
    borders: Vec<PaneBorder>,
    regions: Vec<PaneRegion>,
}

impl PaneLayout {
    pub fn push_border(&mut self, border: PaneBorder) {
        self.borders.push(border);
    }

    pub fn push_region(&mut self, region: PaneRegion) {
        self.regions.push(region);
    }

    pub fn borders(&self) -> &[PaneBorder] {
        &self.borders
    }

    pub fn regions(&self) -> &[PaneRegion] {
        &self.regions
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneBorder {
    axis: PaneBorderAxis,
    col: u16,
    len: u16,
    row: u16,
}

impl PaneBorder {
    pub const fn new(axis: PaneBorderAxis, col: u16, row: u16, len: u16) -> Self {
        Self { axis, col, len, row }
    }

    pub const fn axis(&self) -> PaneBorderAxis {
        self.axis
    }

    pub const fn col(&self) -> u16 {
        self.col
    }

    pub const fn len(&self) -> u16 {
        self.len
    }

    pub const fn row(&self) -> u16 {
        self.row
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneBorderAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneRegion {
    col: u16,
    cols: u16,
    focus_seq: u64,
    id: PaneId,
    row: u16,
    rows: u16,
}

impl PaneRegion {
    pub const fn new(id: PaneId, col: u16, row: u16, cols: u16, rows: u16, focus_seq: u64) -> Self {
        Self {
            col,
            cols,
            focus_seq,
            id,
            row,
            rows,
        }
    }

    pub const fn id(&self) -> &PaneId {
        &self.id
    }

    pub const fn focus_seq(&self) -> u64 {
        self.focus_seq
    }

    pub const fn col(&self) -> u16 {
        self.col
    }

    pub const fn cols(&self) -> u16 {
        self.cols
    }

    pub const fn row(&self) -> u16 {
        self.row
    }

    pub const fn rows(&self) -> u16 {
        self.rows
    }

    pub const fn contains(&self, row: u16, col: u16) -> bool {
        let Some(end_row) = self.row.checked_add(self.rows) else {
            return false;
        };
        let Some(end_col) = self.col.checked_add(self.cols) else {
            return false;
        };

        row >= self.row && row < end_row && col >= self.col && col < end_col
    }

    pub fn is_adjacent_to(&self, other: &Self, direction: PaneFocusDirection) -> bool {
        // Muxr pane regions exclude the separator cell, so visible neighbors have a one-cell gap where Zellij's
        // frame-inclusive pane geometry uses exact edge equality.
        match direction {
            PaneFocusDirection::Left => self.is_directly_left_of(other) && self.horizontally_overlaps_with(other),
            PaneFocusDirection::Right => self.is_directly_right_of(other) && self.horizontally_overlaps_with(other),
            PaneFocusDirection::Up => self.is_directly_above(other) && self.vertically_overlaps_with(other),
            PaneFocusDirection::Down => self.is_directly_below(other) && self.vertically_overlaps_with(other),
        }
    }

    fn is_directly_left_of(&self, other: &Self) -> bool {
        Self::edges_are_adjacent(self.end_col(), u32::from(other.col))
    }

    fn is_directly_right_of(&self, other: &Self) -> bool {
        Self::edges_are_adjacent(other.end_col(), u32::from(self.col))
    }

    fn is_directly_above(&self, other: &Self) -> bool {
        Self::edges_are_adjacent(self.end_row(), u32::from(other.row))
    }

    fn is_directly_below(&self, other: &Self) -> bool {
        Self::edges_are_adjacent(other.end_row(), u32::from(self.row))
    }

    fn horizontally_overlaps_with(&self, other: &Self) -> bool {
        Self::ranges_overlap(
            u32::from(self.row),
            u32::from(self.rows),
            u32::from(other.row),
            u32::from(other.rows),
        )
    }

    fn vertically_overlaps_with(&self, other: &Self) -> bool {
        Self::ranges_overlap(
            u32::from(self.col),
            u32::from(self.cols),
            u32::from(other.col),
            u32::from(other.cols),
        )
    }

    fn end_col(&self) -> u32 {
        u32::from(self.col).saturating_add(u32::from(self.cols))
    }

    fn end_row(&self) -> u32 {
        u32::from(self.row).saturating_add(u32::from(self.rows))
    }

    fn edges_are_adjacent(edge: u32, start: u32) -> bool {
        edge == start || edge.checked_add(1) == Some(start)
    }

    const fn ranges_overlap(first_start: u32, first_len: u32, second_start: u32, second_len: u32) -> bool {
        let first_end = first_start.saturating_add(first_len);
        let second_end = second_start.saturating_add(second_len);

        first_start < second_end && second_start < first_end
    }
}
