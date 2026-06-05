use serde::Serialize;

use super::LayoutSnapshot;
use super::PaneRegionsSnapshot;
use super::TerminalSize;
use crate::SessionName;

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct AttachRequest {
    pub session: SessionName,
    pub terminal_size: TerminalSize,
}

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct AttachAccepted {
    pub layout: LayoutSnapshot,
    pub pane_regions: PaneRegionsSnapshot,
}
