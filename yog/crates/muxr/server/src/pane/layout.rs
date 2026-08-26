use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::TerminalSize;
use rootcause::report;

use crate::pane::borders::PaneBorder;
use crate::pane::borders::PaneBorderAxis;
use crate::pane::split::PaneSplitAxis;
use crate::pane::split::PaneSplitRatio;
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
    pub fn containment(self, position: PanePosition) -> PaneAreaContainment {
        let row = u32::from(position.row);
        let col = u32::from(position.col);

        if row >= u32::from(self.origin.row)
            && row < self.end_row_exclusive()
            && col >= u32::from(self.origin.col)
            && col < self.end_col_exclusive()
        {
            PaneAreaContainment::Inside
        } else {
            PaneAreaContainment::Outside
        }
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
pub enum PaneAreaContainment {
    Inside,
    Outside,
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

    pub fn single_pane(pane_id: PaneId, focus_seq: u64, size: &TerminalSize) -> Self {
        let region = PaneRegion {
            area: PaneArea {
                origin: PanePosition { row: 0, col: 0 },
                size: PaneSize {
                    rows: size.rows(),
                    cols: size.cols(),
                },
            },
            focus_seq,
            id: pane_id,
        };
        Self {
            borders: Vec::new(),
            regions: vec![region],
        }
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
        if content_rows < 2 {
            self.append_collapsed_split(first, second, area);
            return Ok(());
        }
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
        if content_cols < 2 {
            self.append_collapsed_split(first, second, area);
            return Ok(());
        }
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

    fn append_collapsed_split(&mut self, first: &PaneTree, second: &PaneTree, area: PaneArea) {
        // A split with fewer than two content cells cannot show both children and a border. Keep the most recently
        // focused descendant visible instead of resizing a runtime into an impossible collapsed pane.
        let first_pane = first.last_focused_pane();
        let second_pane = second.last_focused_pane();
        let pane = if first_pane.focus_seq >= second_pane.focus_seq {
            first_pane
        } else {
            second_pane
        };
        self.push_region(PaneRegion {
            area,
            focus_seq: pane.focus_seq,
            id: pane.id,
        });
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneRegion {
    pub area: PaneArea,
    pub focus_seq: u64,
    pub id: PaneId,
}

impl PaneRegion {
    pub fn containment(&self, position: PanePosition) -> PaneAreaContainment {
        self.area.containment(position)
    }
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;
    use crate::state::Pane;
    use crate::state::PaneState;

    #[test]
    fn test_pane_layout_single_pane_when_requested_covers_full_size_without_borders() -> rootcause::Result<()> {
        let pane_id = PaneId::new(7)?;
        let layout = PaneLayout::single_pane(pane_id, 3, &TerminalSize::new(80, 24)?);

        assert_that!(layout.borders(), eq(&[]));
        assert_that!(
            layout.regions(),
            eq(&[PaneRegion {
                area: PaneArea {
                    origin: PanePosition { row: 0, col: 0 },
                    size: PaneSize { rows: 24, cols: 80 },
                },
                focus_seq: 3,
                id: pane_id,
            }])
        );
        Ok(())
    }

    #[test]
    fn test_pane_layout_from_pane_tree_when_nested_split_exists_preserves_nested_border_ownership()
    -> rootcause::Result<()> {
        let layout = PaneLayout::from_pane_tree(
            &PaneTree::Split {
                axis: PaneSplitAxis::Vertical,
                first_ratio: PaneSplitRatio::new(500)?,
                first: Box::new(PaneTree::Pane(self::pane(1, 1)?)),
                second: Box::new(PaneTree::Split {
                    axis: PaneSplitAxis::Horizontal,
                    first_ratio: PaneSplitRatio::new(500)?,
                    first: Box::new(PaneTree::Pane(self::pane(2, 2)?)),
                    second: Box::new(PaneTree::Pane(self::pane(3, 3)?)),
                }),
            },
            &TerminalSize::new(80, 24)?,
        )?;

        let borders = layout.borders();
        assert_that!(borders.len(), eq(2));
        let horizontal_border = borders
            .iter()
            .find(|border| {
                (border.axis(), border.col(), border.row(), border.len()) == (PaneBorderAxis::Horizontal, 41, 12, 39)
            })
            .ok_or_else(|| report!("expected nested horizontal split border"))?;
        let vertical_border = borders
            .iter()
            .find(|border| {
                (border.axis(), border.col(), border.row(), border.len()) == (PaneBorderAxis::Vertical, 40, 0, 24)
            })
            .ok_or_else(|| report!("expected nested vertical split border"))?;
        assert_that!(
            horizontal_border.ownership(PanePosition { row: 12, col: 41 }, PaneId::new(2)?),
            eq(crate::pane::borders::BorderCellOwner::Owned)
        );
        assert_that!(
            horizontal_border.ownership(PanePosition { row: 12, col: 41 }, PaneId::new(3)?),
            eq(crate::pane::borders::BorderCellOwner::Owned)
        );
        assert_that!(
            vertical_border.ownership(PanePosition { row: 12, col: 40 }, PaneId::new(1)?),
            eq(crate::pane::borders::BorderCellOwner::Owned)
        );
        assert_that!(
            vertical_border.ownership(PanePosition { row: 12, col: 40 }, PaneId::new(2)?),
            eq(crate::pane::borders::BorderCellOwner::Owned)
        );
        assert_that!(
            vertical_border.ownership(PanePosition { row: 13, col: 40 }, PaneId::new(3)?),
            eq(crate::pane::borders::BorderCellOwner::Owned)
        );
        Ok(())
    }

    #[test]
    fn test_pane_layout_from_pane_tree_when_nested_split_collapses_keeps_focused_visible() -> rootcause::Result<()> {
        let layout = PaneLayout::from_pane_tree(
            &PaneTree::Split {
                axis: PaneSplitAxis::Vertical,
                first_ratio: PaneSplitRatio::new(500)?,
                first: Box::new(PaneTree::Pane(self::pane(1, 1)?)),
                second: Box::new(PaneTree::Split {
                    axis: PaneSplitAxis::Horizontal,
                    first_ratio: PaneSplitRatio::new(500)?,
                    first: Box::new(PaneTree::Pane(self::pane(2, 2)?)),
                    second: Box::new(PaneTree::Pane(self::pane(3, 3)?)),
                }),
            },
            &TerminalSize::new(5, 1)?,
        )?;

        assert_that!(
            layout.regions(),
            eq(&[
                PaneRegion {
                    area: PaneArea {
                        origin: PanePosition { row: 0, col: 0 },
                        size: PaneSize { rows: 1, cols: 2 },
                    },
                    focus_seq: 1,
                    id: PaneId::new(1)?,
                },
                PaneRegion {
                    area: PaneArea {
                        origin: PanePosition { row: 0, col: 3 },
                        size: PaneSize { rows: 1, cols: 2 },
                    },
                    focus_seq: 3,
                    id: PaneId::new(3)?,
                },
            ])
        );
        assert_that!(layout.borders().len(), eq(1));
        Ok(())
    }

    #[test]
    fn test_pane_layout_from_pane_tree_when_vertical_split_collapses_keeps_focused_visible() -> rootcause::Result<()> {
        let layout = PaneLayout::from_pane_tree(
            &PaneTree::Split {
                axis: PaneSplitAxis::Vertical,
                first_ratio: PaneSplitRatio::new(500)?,
                first: Box::new(PaneTree::Pane(self::pane(1, 1)?)),
                second: Box::new(PaneTree::Pane(self::pane(2, 2)?)),
            },
            &TerminalSize::new(2, 4)?,
        )?;

        assert_that!(
            layout.regions(),
            eq(&[PaneRegion {
                area: PaneArea {
                    origin: PanePosition { row: 0, col: 0 },
                    size: PaneSize { rows: 4, cols: 2 },
                },
                focus_seq: 2,
                id: PaneId::new(2)?,
            }])
        );
        assert_that!(layout.borders(), eq(&[]));
        Ok(())
    }

    #[test]
    fn test_pane_layout_from_pane_tree_when_collapsed_split_first_pane_has_focus_keeps_first_visible()
    -> rootcause::Result<()> {
        let layout = PaneLayout::from_pane_tree(
            &PaneTree::Split {
                axis: PaneSplitAxis::Vertical,
                first_ratio: PaneSplitRatio::new(500)?,
                first: Box::new(PaneTree::Pane(self::pane(1, 3)?)),
                second: Box::new(PaneTree::Pane(self::pane(2, 2)?)),
            },
            &TerminalSize::new(2, 4)?,
        )?;

        assert_that!(
            layout.regions(),
            eq(&[PaneRegion {
                area: PaneArea {
                    origin: PanePosition { row: 0, col: 0 },
                    size: PaneSize { rows: 4, cols: 2 },
                },
                focus_seq: 3,
                id: PaneId::new(1)?,
            }])
        );
        assert_that!(layout.borders(), eq(&[]));
        Ok(())
    }

    fn pane(id: u32, focus_seq: u64) -> rootcause::Result<Pane> {
        Ok(Pane {
            attention_state: crate::state::PaneAttentionState::Idle,
            cmd_label: "zsh".to_owned(),
            cwd: "/tmp".to_owned(),
            focus_seq,
            id: PaneId::new(id)?,
            started_at: focus_seq,
            state: PaneState::Running,
            title: "zsh".to_owned(),
        })
    }
}
