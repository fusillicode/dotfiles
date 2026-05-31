use muxr_core::ClientMouseEvent;
use muxr_core::ClientMousePosition;
use muxr_core::PaneScrollDirection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScrollAction {
    pub direction: PaneScrollDirection,
    pub position: ClientMousePosition,
}

pub const fn scroll_action(event: ClientMouseEvent) -> Option<ScrollAction> {
    if event.button() & 64 == 0 {
        return None;
    }

    match event.button() & 0b11 {
        0 => Some(ScrollAction {
            direction: PaneScrollDirection::Up,
            position: event.position(),
        }),
        1 => Some(ScrollAction {
            direction: PaneScrollDirection::Down,
            position: event.position(),
        }),
        _ => None,
    }
}
