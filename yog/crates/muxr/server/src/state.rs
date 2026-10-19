pub use crate::state::pane::Pane;
pub use crate::state::pane::PaneAttentionState;
pub use crate::state::pane::PaneState;
pub use crate::state::pane::PaneTree;
pub use crate::state::pane::PaneTreeRightPane;
pub use crate::state::session::PaneMetadataSync;
pub use crate::state::session::PaneSnapshotFields;
pub use crate::state::session::SessionLayout;
pub use crate::state::session::SessionMetadata;
pub use crate::state::tab::Tab;
mod pane;

pub mod persisted;
mod session;
mod tab;
pub const VERSION: u16 = 4;

#[cfg(test)]
pub mod test_helpers {
    pub use crate::state::session::test_helpers::*;
}
