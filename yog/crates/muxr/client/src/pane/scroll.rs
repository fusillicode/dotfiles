use muxr_core::ClientMouseEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseWheelEvent {
    Other,
    Wheel,
}

impl From<ClientMouseEvent> for MouseWheelEvent {
    fn from(event: ClientMouseEvent) -> Self {
        if event.button & 64 != 0 && matches!(event.button & 0b11, 0 | 1) {
            Self::Wheel
        } else {
            Self::Other
        }
    }
}
