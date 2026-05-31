use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::ClientMousePosition;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalMouseAction {
    FocusAndSelectionStart(ClientMousePosition),
    SelectionEnd(ClientMousePosition),
    SelectionUpdate(ClientMousePosition),
}

pub fn local_mouse_action(event: ClientMouseEvent) -> Option<LocalMouseAction> {
    let position = event.position();
    if event.phase() == ClientMouseEventPhase::Release {
        return Some(LocalMouseAction::SelectionEnd(position));
    }
    if event.button() & (64 | 0b11) != 0 {
        return None;
    }
    if event.button() & 32 != 0 {
        return Some(LocalMouseAction::SelectionUpdate(position));
    }
    Some(LocalMouseAction::FocusAndSelectionStart(position))
}
