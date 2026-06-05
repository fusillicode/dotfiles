use serde::Serialize;

#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct ClientMousePosition {
    /// Zero-based row in the client viewport.
    pub row: u16,
    /// Zero-based column in the client viewport.
    pub col: u16,
}

/// Press or release phase for an SGR mouse event captured by the muxr client.
#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum ClientMouseEventPhase {
    /// Button press, wheel, or button-motion event.
    Press,
    /// Button release event.
    Release,
}

/// Mouse event captured from the outer terminal before server-side pane translation.
#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct ClientMouseEvent {
    /// SGR button code, including modifier, wheel, and motion bits.
    pub button: u16,
    /// Press/motion/wheel or release phase.
    pub phase: ClientMouseEventPhase,
    /// Position in client viewport coordinates.
    pub position: ClientMousePosition,
}
