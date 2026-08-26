use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::ClientMousePosition;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalMouseAction {
    FocusAndSelectionStart(ClientMousePosition),
    SelectionEnd(ClientMousePosition),
    SelectionUpdate(ClientMousePosition),
}

impl LocalMouseAction {
    pub fn from_event(event: ClientMouseEvent) -> Option<Self> {
        let position = event.position;
        if event.phase == ClientMouseEventPhase::Release {
            return Some(Self::SelectionEnd(position));
        }
        if event.button & (64 | 0b11) != 0 {
            return None;
        }
        if event.button & 32 != 0 {
            return Some(Self::SelectionUpdate(position));
        }
        Some(Self::FocusAndSelectionStart(position))
    }
}
