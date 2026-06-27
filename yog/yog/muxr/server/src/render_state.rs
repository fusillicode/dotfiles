use crate::session::tracing::ClientEventSendFailure;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ClientRenderDmg {
    #[default]
    Clean,
    Dirty,
}

impl ClientRenderDmg {
    pub const fn mark_dirty(&mut self) {
        *self = Self::Dirty;
    }

    pub const fn include_dmg(&mut self, dmg: Self) {
        if matches!(dmg, Self::Dirty) {
            self.mark_dirty();
        }
    }

    pub const fn include_signal(&mut self, signal: PaneRenderSignal) {
        self.include_dmg(signal.render_dmg());
    }

    pub const fn clear(&mut self) {
        *self = Self::Clean;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PaneRenderSignal {
    #[default]
    Unchanged,
    DeadlineOnly,
    DirtyAndDeadline,
}

impl PaneRenderSignal {
    pub const fn from_dmg_and_deadline(dmg: ClientRenderDmg, deadline_sync: PaneRenderDeadlineSync) -> Self {
        match (dmg, deadline_sync) {
            (ClientRenderDmg::Dirty, _) => Self::DirtyAndDeadline,
            (ClientRenderDmg::Clean, PaneRenderDeadlineSync::Sync) => Self::DeadlineOnly,
            (ClientRenderDmg::Clean, PaneRenderDeadlineSync::Skip) => Self::Unchanged,
        }
    }

    pub const fn render_dmg(self) -> ClientRenderDmg {
        match self {
            Self::DirtyAndDeadline => ClientRenderDmg::Dirty,
            Self::DeadlineOnly | Self::Unchanged => ClientRenderDmg::Clean,
        }
    }

    pub const fn deadline_sync(self) -> PaneRenderDeadlineSync {
        match self {
            Self::DeadlineOnly | Self::DirtyAndDeadline => PaneRenderDeadlineSync::Sync,
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
