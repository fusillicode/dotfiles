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
pub enum PaneAgentState {
    /// No known agent command is running in the pane.
    #[default]
    NoAgent,
    /// An agent previously needed attention and that attention was acknowledged by focusing the pane.
    Seen,
    /// An agent command is running, with no pending attention signal.
    Busy,
    /// An agent needs attention and has not yet been acknowledged by focusing the pane.
    Unseen,
}
