use serde::Deserialize;
use serde::Serialize;

#[derive(
    rkyv::Archive,
    Clone,
    Copy,
    Debug,
    Default,
    Deserialize,
    rkyv::Deserialize,
    Eq,
    PartialEq,
    Serialize,
    rkyv::Serialize,
)]
pub enum TrackedProcessState {
    /// No configured tracked process is running in the pane.
    #[default]
    None,
    /// A tracked process previously needed attention and that attention was acknowledged by focusing the pane.
    Seen,
    /// A tracked process is running, with no pending attention signal.
    Busy,
    /// A tracked process needs attention and has not yet been acknowledged by focusing the pane.
    Unseen,
}
