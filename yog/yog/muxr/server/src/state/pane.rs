use muxr_core::PaneId;
use muxr_core::PaneSnapshot;
use serde::Deserialize;
use serde::Serialize;

use crate::pane_split::PaneSplitAxis;
use crate::pane_split::PaneSplitRatio;
use crate::pty::PtyExitStatus;

// Pane splits are a tree so a new split mutates only the active pane subtree; a tab-wide axis would reflow siblings.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneTree {
    Pane(Pane),
    Split {
        axis: PaneSplitAxis,
        first_ratio: PaneSplitRatio,
        first: Box<Self>,
        second: Box<Self>,
    },
}

impl PaneTree {
    pub fn pane_count(&self) -> usize {
        match self {
            Self::Pane(_) => 1,
            Self::Split { first, second, .. } => first.pane_count().saturating_add(second.pane_count()),
        }
    }

    pub fn contains_pane(&self, pane_id: &PaneId) -> bool {
        match self {
            Self::Pane(pane) => pane.id == *pane_id,
            Self::Split { first, second, .. } => first.contains_pane(pane_id) || second.contains_pane(pane_id),
        }
    }

    pub fn pane_mut(&mut self, pane_id: &PaneId) -> Option<&mut Pane> {
        match self {
            Self::Pane(pane) if pane.id == *pane_id => Some(pane),
            Self::Pane(_) => None,
            Self::Split { first, second, .. } => first.pane_mut(pane_id).or_else(|| second.pane_mut(pane_id)),
        }
    }

    pub fn append_pane_ids<'a>(&'a self, ids: &mut Vec<&'a str>) {
        match self {
            Self::Pane(pane) => ids.push(pane.id.as_ref()),
            Self::Split { first, second, .. } => {
                first.append_pane_ids(ids);
                second.append_pane_ids(ids);
            }
        }
    }

    pub fn append_panes<'a>(&'a self, panes: &mut Vec<&'a Pane>) {
        match self {
            Self::Pane(pane) => panes.push(pane),
            Self::Split { first, second, .. } => {
                first.append_panes(panes);
                second.append_panes(panes);
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Pane {
    pub command_label: String,
    pub cwd: String,
    pub focus_seq: u64,
    pub id: PaneId,
    pub started_at: u64,
    pub state: PaneState,
    pub title: String,
}

impl Pane {
    pub const fn set_focus_seq(&mut self, focus_seq: u64) {
        self.focus_seq = focus_seq;
    }

    pub fn mark_closed(&mut self, at: u64) {
        self.state = PaneState::Closed { at };
    }

    pub fn mark_process_exited(&mut self, at: u64, status: PtyExitStatus) {
        self.state = PaneState::ProcessExited { at, status };
    }

    pub fn snapshot(&self) -> PaneSnapshot {
        PaneSnapshot {
            cwd: self.cwd.clone(),
            id: self.id.clone(),
            title: self.title.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneState {
    Running,
    Closed { at: u64 },
    ProcessExited { at: u64, status: PtyExitStatus },
}
