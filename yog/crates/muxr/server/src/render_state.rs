use muxr_core::PaneId;
use smallvec::SmallVec;

use crate::session::tracing::ClientEventSendFailure;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ClientRenderDmg {
    #[default]
    Clean,
    Panes(SmallVec<[PaneId; 4]>),
    RegionChanged(SmallVec<[PaneId; 4]>),
    Full,
}

impl ClientRenderDmg {
    pub fn pane(pane_id: PaneId) -> Self {
        Self::Panes(SmallVec::from_slice(&[pane_id]))
    }

    pub fn panes(pane_ids: impl IntoIterator<Item = PaneId>) -> Self {
        let mut pane_ids_out = SmallVec::new();
        for pane_id in pane_ids {
            if !pane_ids_out.contains(&pane_id) {
                pane_ids_out.push(pane_id);
            }
        }
        if pane_ids_out.is_empty() {
            Self::Clean
        } else {
            Self::Panes(pane_ids_out)
        }
    }

    pub fn region_changed(pane_id: PaneId) -> Self {
        Self::RegionChanged(SmallVec::from_slice(&[pane_id]))
    }

    pub fn include_dmg(&mut self, dmg: Self) {
        match (&mut *self, dmg) {
            (Self::Full, _) | (_, Self::Clean) => {}
            (slot, Self::Full) => *slot = Self::Full,
            (Self::Clean, dmg) => *self = dmg,
            (Self::Panes(current), Self::Panes(incoming)) => self::extend_unique(current, incoming),
            (Self::Panes(current), Self::RegionChanged(incoming)) => {
                self::extend_unique(current, incoming);
                *self = Self::RegionChanged(std::mem::take(current));
            }
            (Self::RegionChanged(current), Self::Panes(incoming) | Self::RegionChanged(incoming)) => {
                self::extend_unique(current, incoming);
            }
        }
    }

    pub fn include_signal(&mut self, signal: &PaneRenderSignal) {
        self.include_dmg(signal.render_dmg());
    }

    pub fn clear(&mut self) {
        *self = Self::Clean;
    }

    pub const fn is_clean(&self) -> bool {
        matches!(self, Self::Clean)
    }
}

fn extend_unique(current: &mut SmallVec<[PaneId; 4]>, incoming: SmallVec<[PaneId; 4]>) {
    for pane_id in incoming {
        if !current.contains(&pane_id) {
            current.push(pane_id);
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PaneRenderSignal {
    #[default]
    Unchanged,
    DeadlineOnly,
    DirtyAndDeadline(ClientRenderDmg),
}

impl PaneRenderSignal {
    pub fn from_dmg_and_deadline(dmg: ClientRenderDmg, deadline_sync: PaneRenderDeadlineSync) -> Self {
        match (dmg, deadline_sync) {
            (ClientRenderDmg::Clean, PaneRenderDeadlineSync::Sync) => Self::DeadlineOnly,
            (ClientRenderDmg::Clean, PaneRenderDeadlineSync::Skip) => Self::Unchanged,
            (dmg, _) => Self::DirtyAndDeadline(dmg),
        }
    }

    pub fn render_dmg(&self) -> ClientRenderDmg {
        match self {
            Self::DirtyAndDeadline(dmg) => dmg.clone(),
            Self::DeadlineOnly | Self::Unchanged => ClientRenderDmg::Clean,
        }
    }

    pub const fn deadline_sync(&self) -> PaneRenderDeadlineSync {
        match self {
            Self::DeadlineOnly | Self::DirtyAndDeadline(_) => PaneRenderDeadlineSync::Sync,
            Self::Unchanged => PaneRenderDeadlineSync::Skip,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneRenderDeadlineSync {
    Skip,
    Sync,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PaneInputRenderPriority {
    #[default]
    Bulk,
    Interactive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFreshness {
    Current,
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientEventSendOutcome {
    Sent,
    Failed(ClientEventSendFailure),
}

impl ClientEventSendOutcome {
    pub const fn session_flow(&self) -> ClientSessionFlow {
        match self {
            Self::Sent => ClientSessionFlow::Continue,
            Self::Failed(_) => ClientSessionFlow::Disconnect,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientSessionFlow {
    Continue,
    Disconnect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientLifecycleAction {
    Continue,
    Exit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientSessionSelectBias {
    Output,
    Request,
}
