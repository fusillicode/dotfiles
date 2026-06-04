use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::TerminalSize;
use rootcause::report;

use crate::pane_borders::PaneBorder;
use crate::pane_borders::PaneBorderAxis;
use crate::pane_split::PaneSplitAxis;
use crate::pane_split::PaneSplitRatio;
use crate::state::PaneTree;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PanePosition {
    pub row: u16,
    pub col: u16,
}

impl From<ClientMousePosition> for PanePosition {
    fn from(position: ClientMousePosition) -> Self {
        Self {
            row: position.row,
            col: position.col,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneSize {
    pub rows: u16,
    pub cols: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneArea {
    pub origin: PanePosition,
    pub size: PaneSize,
}

impl PaneArea {
    pub fn contains(self, position: PanePosition) -> bool {
        let row = u32::from(position.row);
        let col = u32::from(position.col);

        row >= u32::from(self.origin.row)
            && row < self.end_row_exclusive()
            && col >= u32::from(self.origin.col)
            && col < self.end_col_exclusive()
    }

    pub const fn end_col(self) -> Option<u16> {
        self.origin.col.checked_add(self.size.cols)
    }

    pub fn end_col_exclusive(self) -> u32 {
        u32::from(self.origin.col).saturating_add(u32::from(self.size.cols))
    }

    pub const fn end_row(self) -> Option<u16> {
        self.origin.row.checked_add(self.size.rows)
    }

    pub fn end_row_exclusive(self) -> u32 {
        u32::from(self.origin.row).saturating_add(u32::from(self.size.rows))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PaneSplitLayout {
    border_axis: PaneBorderAxis,
    border_len: u16,
    border_position: PanePosition,
    first_area: PaneArea,
    second_area: PaneArea,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaneLayout {
    borders: Vec<PaneBorder>,
    regions: Vec<PaneRegion>,
}

impl PaneLayout {
    pub fn from_pane_tree(pane_tree: &PaneTree, size: &TerminalSize) -> rootcause::Result<Self> {
        let mut layout = Self::default();
        let area = PaneArea {
            origin: PanePosition { row: 0, col: 0 },
            size: PaneSize {
                rows: size.rows(),
                cols: size.cols(),
            },
        };
        layout.append_tree(pane_tree, area)?;
        Ok(layout)
    }

    pub fn borders(&self) -> &[PaneBorder] {
        &self.borders
    }

    pub fn regions(&self) -> &[PaneRegion] {
        &self.regions
    }

    fn push_border(&mut self, border: PaneBorder) {
        self.borders.push(border);
    }

    fn push_region(&mut self, region: PaneRegion) {
        self.regions.push(region);
    }

    fn regions_added_since(&self, start: usize) -> rootcause::Result<Vec<PaneRegion>> {
        self.regions.get(start..).map(<[PaneRegion]>::to_vec).ok_or_else(|| {
            report!("muxr pane layout region start outside region list").attach(format!("start={start}"))
        })
    }

    fn append_tree(&mut self, pane_tree: &PaneTree, area: PaneArea) -> rootcause::Result<()> {
        match pane_tree {
            PaneTree::Pane(pane) => {
                self.push_region(PaneRegion {
                    area,
                    focus_seq: pane.focus_seq,
                    id: pane.id,
                });
                Ok(())
            }
            PaneTree::Split {
                axis,
                first_ratio,
                first,
                second,
            } => match axis {
                PaneSplitAxis::Horizontal => self.append_horizontal_split(*first_ratio, first, second, area),
                PaneSplitAxis::Vertical => self.append_vertical_split(*first_ratio, first, second, area),
            },
        }
    }

    fn append_horizontal_split(
        &mut self,
        first_ratio: PaneSplitRatio,
        first: &PaneTree,
        second: &PaneTree,
        area: PaneArea,
    ) -> rootcause::Result<()> {
        let content_rows = area
            .size
            .rows
            .checked_sub(1)
            .ok_or_else(|| report!("muxr terminal is too small for horizontal pane border"))?;
        let (first_rows, second_rows) = first_ratio.split_lengths(content_rows)?;
        let border_row = area
            .origin
            .row
            .checked_add(first_rows)
            .ok_or_else(|| report!("muxr pane border row overflowed"))?;
        let second_row = area
            .origin
            .row
            .checked_add(first_rows)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| report!("muxr pane split row overflowed"))?;
        let first_area = PaneArea {
            origin: area.origin,
            size: PaneSize {
                rows: first_rows,
                cols: area.size.cols,
            },
        };
        let second_area = PaneArea {
            origin: PanePosition {
                row: second_row,
                col: area.origin.col,
            },
            size: PaneSize {
                rows: second_rows,
                cols: area.size.cols,
            },
        };
        self.append_split(
            first,
            second,
            PaneSplitLayout {
                border_axis: PaneBorderAxis::Horizontal,
                border_len: area.size.cols,
                border_position: PanePosition {
                    row: border_row,
                    col: area.origin.col,
                },
                first_area,
                second_area,
            },
        )
    }

    fn append_vertical_split(
        &mut self,
        first_ratio: PaneSplitRatio,
        first: &PaneTree,
        second: &PaneTree,
        area: PaneArea,
    ) -> rootcause::Result<()> {
        let content_cols = area
            .size
            .cols
            .checked_sub(1)
            .ok_or_else(|| report!("muxr terminal is too small for vertical pane border"))?;
        let (first_cols, second_cols) = first_ratio.split_lengths(content_cols)?;
        let border_col = area
            .origin
            .col
            .checked_add(first_cols)
            .ok_or_else(|| report!("muxr pane border col overflowed"))?;
        let second_col = area
            .origin
            .col
            .checked_add(first_cols)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| report!("muxr pane split col overflowed"))?;
        let first_area = PaneArea {
            origin: area.origin,
            size: PaneSize {
                rows: area.size.rows,
                cols: first_cols,
            },
        };
        let second_area = PaneArea {
            origin: PanePosition {
                row: area.origin.row,
                col: second_col,
            },
            size: PaneSize {
                rows: area.size.rows,
                cols: second_cols,
            },
        };
        self.append_split(
            first,
            second,
            PaneSplitLayout {
                border_axis: PaneBorderAxis::Vertical,
                border_len: area.size.rows,
                border_position: PanePosition {
                    row: area.origin.row,
                    col: border_col,
                },
                first_area,
                second_area,
            },
        )
    }

    fn append_split(
        &mut self,
        first: &PaneTree,
        second: &PaneTree,
        split_layout: PaneSplitLayout,
    ) -> rootcause::Result<()> {
        let first_region_start = self.regions().len();
        self.append_tree(first, split_layout.first_area)?;
        let first_regions = self.regions_added_since(first_region_start)?;
        let second_region_start = self.regions().len();
        self.append_tree(second, split_layout.second_area)?;
        let second_regions = self.regions_added_since(second_region_start)?;
        self.push_border(PaneBorder::with_adjacent_regions(
            split_layout.border_axis,
            split_layout.border_position,
            split_layout.border_len,
            &first_regions,
            &second_regions,
        )?);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneRegion {
    pub area: PaneArea,
    pub focus_seq: u64,
    pub id: PaneId,
}

impl PaneRegion {
    pub fn contains(&self, position: PanePosition) -> bool {
        self.area.contains(position)
    }
}
