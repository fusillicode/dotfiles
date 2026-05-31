use muxr_core::PaneId;
use muxr_core::PaneSnapshot;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

use crate::geometry::PaneBorder;
use crate::geometry::PaneBorderAxis;
use crate::geometry::PaneLayout;
use crate::geometry::PaneRegion;
use crate::pane_split::PaneSplitAxis;
use crate::pane_split::PaneSplitRatio;
use crate::pty::PtyExitStatus;

// Pane splits are a tree so a new split mutates only the active leaf; a tab-wide axis would reflow siblings.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneNode {
    Leaf {
        pane: Pane,
    },
    Split {
        axis: PaneSplitAxis,
        first_ratio: PaneSplitRatio,
        first: Box<Self>,
        second: Box<Self>,
    },
}

impl PaneNode {
    pub const fn leaf(pane: Pane) -> Self {
        Self::Leaf { pane }
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Pane {
    command_label: String,
    cwd: String,
    exit_status: Option<PtyExitStatus>,
    exited_at: Option<u64>,
    focus_seq: u64,
    id: PaneId,
    started_at: u64,
    title: String,
}

impl Pane {
    pub fn new(id: PaneId, command_label: String, cwd: String, started_at: u64, focus_seq: u64) -> Self {
        Self {
            command_label: command_label.clone(),
            cwd,
            exit_status: None,
            exited_at: None,
            focus_seq,
            id,
            started_at,
            title: command_label,
        }
    }

    pub const fn id(&self) -> &PaneId {
        &self.id
    }

    pub const fn focus_seq(&self) -> u64 {
        self.focus_seq
    }

    pub const fn set_focus_seq(&mut self, focus_seq: u64) {
        self.focus_seq = focus_seq;
    }

    pub fn mark_exited(&mut self, exited_at: u64, exit_status: Option<PtyExitStatus>) {
        self.exited_at = Some(exited_at);
        self.exit_status = exit_status;
    }

    pub fn snapshot(&self) -> PaneSnapshot {
        PaneSnapshot::new(self.id.clone(), self.title.clone())
    }
}
