use serde::Serialize;

#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum PaneScrollDirection {
    Down,
    Up,
}

/// Outcome for a single-line pane scroll request.
#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum PaneScrollLineMove {
    Moved,
    Unchanged,
}

impl PaneScrollLineMove {
    #[must_use]
    pub const fn from_scrolled(scrolled: bool) -> Self {
        if scrolled { Self::Moved } else { Self::Unchanged }
    }

    #[must_use]
    pub const fn scrolled(self) -> bool {
        matches!(self, Self::Moved)
    }
}
