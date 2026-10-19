use serde::Serialize;

use crate::SessionName;
use crate::protocol::LayoutSnapshot;
use crate::protocol::PaneRegionsSnapshot;
use crate::protocol::TerminalSize;

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
