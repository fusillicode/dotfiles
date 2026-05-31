use muxr_core::PaneId;
use rootcause::report;

use crate::layout::Pane;
use crate::layout::PaneNode;
use crate::layout::PaneResizeDirection;
use crate::layout::PaneSplitAxis;
use crate::layout::PaneSplitRatio;
use crate::layout::PaneSplitResize;
use crate::layout::region::PaneBorder;
use crate::layout::region::PaneBorderAxis;
use crate::layout::region::PaneLayout;
use crate::layout::region::PaneRegion;

impl PaneNode {
    pub const fn leaf(pane: Pane) -> Self {
        Self::Leaf { pane }
    }

    pub fn split_pane(
        &mut self,
        pane_id: &PaneId,
        new_pane: &Pane,
        split_axis: PaneSplitAxis,
    ) -> rootcause::Result<bool> {
        match self {
            Self::Leaf { pane } if pane.id == *pane_id => {
                let old_pane = pane.clone();
                *self = Self::Split {
                    axis: split_axis,
                    first_ratio: PaneSplitRatio::balanced(),
                    first: Box::new(Self::leaf(old_pane)),
                    second: Box::new(Self::leaf(new_pane.clone())),
                };
                Ok(true)
            }
            Self::Leaf { .. } => Ok(false),
            Self::Split { first, second, .. } => {
                if first.split_pane(pane_id, new_pane, split_axis)? {
                    return Ok(true);
                }
                second.split_pane(pane_id, new_pane, split_axis)
            }
        }
    }

    pub fn resize_pane(&mut self, pane_id: &PaneId, direction: PaneResizeDirection) -> rootcause::Result<bool> {
        match self {
            Self::Leaf { .. } => Ok(false),
            Self::Split {
                axis,
                first_ratio,
                first,
                second,
            } => {
                let child_resized = if first.contains_pane(pane_id) {
                    first.resize_pane(pane_id, direction)?
                } else if second.contains_pane(pane_id) {
                    second.resize_pane(pane_id, direction)?
                } else {
                    return Ok(false);
                };
                if child_resized {
                    return Ok(true);
                }

                let Some(resize) = PaneSplitResize::for_direction(*axis, direction) else {
                    return Ok(false);
                };
                let resized_ratio = first_ratio.resized(resize);
                if resized_ratio == *first_ratio {
                    return Ok(false);
                }

                *first_ratio = resized_ratio;
                Ok(true)
            }
        }
    }

    pub fn remove_pane(&mut self, pane_id: &PaneId) -> rootcause::Result<PaneId> {
        let Some(fallback_pane) = self.remove_leaf(pane_id)? else {
            return Err(report!("muxr pane is missing from server layout").attach(format!("pane_id={pane_id}")));
        };
        Ok(fallback_pane)
    }

    fn remove_leaf(&mut self, pane_id: &PaneId) -> rootcause::Result<Option<PaneId>> {
        match self {
            Self::Leaf { pane } if pane.id == *pane_id => {
                Err(report!("muxr cannot remove a pane leaf without a sibling").attach(format!("pane_id={pane_id}")))
            }
            Self::Split { first, second, .. } if first.contains_pane(pane_id) => {
                if first.pane_count() == 1 {
                    let replacement = (**second).clone();
                    let fallback_pane = replacement.first_pane_id();
                    *self = replacement;
                    Ok(Some(fallback_pane))
                } else {
                    first.remove_leaf(pane_id)
                }
            }
            Self::Split { first, second, .. } if second.contains_pane(pane_id) => {
                if second.pane_count() == 1 {
                    let replacement = (**first).clone();
                    let fallback_pane = replacement.first_pane_id();
                    *self = replacement;
                    Ok(Some(fallback_pane))
                } else {
                    second.remove_leaf(pane_id)
                }
            }
            Self::Leaf { .. } | Self::Split { .. } => Ok(None),
        }
    }

    pub fn pane_count(&self) -> usize {
        match self {
            Self::Leaf { .. } => 1,
            Self::Split { first, second, .. } => first.pane_count().saturating_add(second.pane_count()),
        }
    }

    pub fn contains_pane(&self, pane_id: &PaneId) -> bool {
        match self {
            Self::Leaf { pane } => pane.id == *pane_id,
            Self::Split { first, second, .. } => first.contains_pane(pane_id) || second.contains_pane(pane_id),
        }
    }

    fn first_pane_id(&self) -> PaneId {
        match self {
            Self::Leaf { pane } => pane.id.clone(),
            Self::Split { first, .. } => first.first_pane_id(),
        }
    }

    pub fn pane_mut(&mut self, pane_id: &PaneId) -> Option<&mut Pane> {
        match self {
            Self::Leaf { pane } if pane.id == *pane_id => Some(pane),
            Self::Leaf { .. } => None,
            Self::Split { first, second, .. } => first.pane_mut(pane_id).or_else(|| second.pane_mut(pane_id)),
        }
    }

    pub fn append_pane_ids<'a>(&'a self, ids: &mut Vec<&'a str>) {
        match self {
            Self::Leaf { pane } => ids.push(pane.id.as_ref()),
            Self::Split { first, second, .. } => {
                first.append_pane_ids(ids);
                second.append_pane_ids(ids);
            }
        }
    }

    pub fn append_panes<'a>(&'a self, panes: &mut Vec<&'a Pane>) {
        match self {
            Self::Leaf { pane } => panes.push(pane),
            Self::Split { first, second, .. } => {
                first.append_panes(panes);
                second.append_panes(panes);
            }
        }
    }

    pub fn append_layout(
        &self,
        row: u16,
        col: u16,
        rows: u16,
        cols: u16,
        layout: &mut PaneLayout,
    ) -> rootcause::Result<()> {
        match self {
            Self::Leaf { pane } => {
                layout.push_region(PaneRegion::new(pane.id.clone(), col, row, cols, rows, pane.focus_seq));
                Ok(())
            }
            Self::Split {
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
                    first.append_layout(row, col, first_rows, cols, layout)?;
                    layout.push_border(PaneBorder::new(PaneBorderAxis::Horizontal, col, border_row, cols));
                    second.append_layout(second_row, col, second_rows, cols, layout)
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
                    first.append_layout(row, col, rows, first_cols, layout)?;
                    layout.push_border(PaneBorder::new(PaneBorderAxis::Vertical, border_col, row, rows));
                    second.append_layout(row, second_col, rows, second_cols, layout)
                }
            },
        }
    }
}
