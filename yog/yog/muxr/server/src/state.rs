pub use self::pane::Pane;
pub use self::pane::PaneAttentionState;
pub use self::pane::PaneState;
pub use self::pane::PaneTree;
pub use self::session::SessionLayout;
pub use self::session::SessionMetadata;
pub use self::tab::Tab;

mod pane;
pub mod persisted;
mod session;
mod tab;

pub const VERSION: u16 = 4;
