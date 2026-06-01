use muxr_core::PaneId;
use muxr_core::TerminalSize;
use rootcause::report;

use crate::pane_borders::PaneBorder;
use crate::pane_borders::PaneBorderAxis;
use crate::pane_split::PaneSplitAxis;
use crate::state::PaneNode;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaneLayout {
    borders: Vec<PaneBorder>,
    regions: Vec<PaneRegion>,
}

impl PaneLayout {
    pub fn from_pane_tree(pane_tree: &PaneNode, size: &TerminalSize) -> rootcause::Result<Self> {
        let mut layout = Self::default();
        layout.append_pane_tree(pane_tree, 0, 0, size.rows(), size.cols())?;
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

    fn append_pane_tree(
        &mut self,
        pane_tree: &PaneNode,
        row: u16,
        col: u16,
        rows: u16,
        cols: u16,
    ) -> rootcause::Result<()> {
        match pane_tree {
            PaneNode::Leaf { pane } => {
                self.push_region(PaneRegion::new(
                    pane.id().clone(),
                    col,
                    row,
                    cols,
                    rows,
                    pane.focus_seq(),
                ));
                Ok(())
            }
            PaneNode::Split {
                axis,
                first_ratio,
                first,
                second,
            } => match axis {
                PaneSplitAxis::Horizontal => {
                    let content_rows = rows
                        .checked_sub(1)
                        .ok_or_else(|| report!("muxr terminal is too small for horizontal pane border"))?;
                    let (first_rows, second_rows) = first_ratio.split_lengths(content_rows)?;
                    let border_row = row
                        .checked_add(first_rows)
                        .ok_or_else(|| report!("muxr pane border row overflowed"))?;
                    let second_row = row
                        .checked_add(first_rows)
                        .and_then(|value| value.checked_add(1))
                        .ok_or_else(|| report!("muxr pane split row overflowed"))?;
                    let first_region_start = self.regions().len();
                    self.append_pane_tree(first, row, col, first_rows, cols)?;
                    let first_regions = self.regions_added_since(first_region_start)?;
                    let second_region_start = self.regions().len();
                    self.append_pane_tree(second, second_row, col, second_rows, cols)?;
                    let second_regions = self.regions_added_since(second_region_start)?;
                    self.push_border(PaneBorder::with_adjacent_regions(
                        PaneBorderAxis::Horizontal,
                        col,
                        border_row,
                        cols,
                        &first_regions,
                        &second_regions,
                    )?);
                    Ok(())
                }
                PaneSplitAxis::Vertical => {
                    let content_cols = cols
                        .checked_sub(1)
                        .ok_or_else(|| report!("muxr terminal is too small for vertical pane border"))?;
                    let (first_cols, second_cols) = first_ratio.split_lengths(content_cols)?;
                    let border_col = col
                        .checked_add(first_cols)
                        .ok_or_else(|| report!("muxr pane border col overflowed"))?;
                    let second_col = col
                        .checked_add(first_cols)
                        .and_then(|value| value.checked_add(1))
                        .ok_or_else(|| report!("muxr pane split col overflowed"))?;
                    let first_region_start = self.regions().len();
                    self.append_pane_tree(first, row, col, rows, first_cols)?;
                    let first_regions = self.regions_added_since(first_region_start)?;
                    let second_region_start = self.regions().len();
                    self.append_pane_tree(second, row, second_col, rows, second_cols)?;
                    let second_regions = self.regions_added_since(second_region_start)?;
                    self.push_border(PaneBorder::with_adjacent_regions(
                        PaneBorderAxis::Vertical,
                        border_col,
                        row,
                        rows,
                        &first_regions,
                        &second_regions,
                    )?);
                    Ok(())
                }
            },
        }
    }
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
}
