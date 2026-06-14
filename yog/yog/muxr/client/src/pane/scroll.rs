use muxr_core::ClientMouseEvent;

pub const fn is_wheel_event(event: ClientMouseEvent) -> bool {
    event.button & 64 != 0 && matches!(event.button & 0b11, 0 | 1)
}
