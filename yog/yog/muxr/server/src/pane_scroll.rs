use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::PaneScrollDirection;
use muxr_core::ServerEvent;

use crate::pane_runtime::PaneRuntimes;

const FAUX_SCROLL_LINES_PER_WHEEL_EVENT: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneScrollAmount {
    Line,
    Wheel,
}

pub struct PaneScrollLineRequestOutcome {
    pub event: ServerEvent,
    pub render_dirty: bool,
}

pub const fn scroll_pane_line_result(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    scrolled: bool,
) -> PaneScrollLineRequestOutcome {
    PaneScrollLineRequestOutcome {
        event: ServerEvent::ScrollPaneLineResult {
            position,
            direction,
            scrolled,
        },
        // Edge-drag autoscroll can outpace render IO; keep viewport changes coalesced on the render tick.
        render_dirty: scrolled,
    }
}

pub fn scroll_pane(
    pane_id: PaneId,
    direction: PaneScrollDirection,
    amount: PaneScrollAmount,
    runtimes: &PaneRuntimes,
) -> rootcause::Result<bool> {
    match amount {
        PaneScrollAmount::Line => runtimes.handle(pane_id)?.scroll_one_line(direction),
        PaneScrollAmount::Wheel => runtimes.handle(pane_id)?.scroll(direction),
    }
}

pub fn faux_scroll_input_bytes(direction: PaneScrollDirection, application_cursor: bool) -> Vec<u8> {
    let sequence = self::faux_scroll_sequence(direction, application_cursor);
    let mut bytes = Vec::with_capacity(sequence.len().saturating_mul(FAUX_SCROLL_LINES_PER_WHEEL_EVENT));
    for _ in 0..FAUX_SCROLL_LINES_PER_WHEEL_EVENT {
        bytes.extend_from_slice(sequence);
    }
    bytes
}

const fn faux_scroll_sequence(direction: PaneScrollDirection, application_cursor: bool) -> &'static [u8] {
    match (direction, application_cursor) {
        (PaneScrollDirection::Up, false) => b"\x1b[A",
        (PaneScrollDirection::Down, false) => b"\x1b[B",
        (PaneScrollDirection::Up, true) => b"\x1bOA",
        (PaneScrollDirection::Down, true) => b"\x1bOB",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_faux_scroll_input_bytes_when_application_cursor_mode_is_disabled_uses_csi_arrows() {
        pretty_assertions::assert_eq!(
            faux_scroll_input_bytes(PaneScrollDirection::Up, false),
            b"\x1b[A\x1b[A\x1b[A".to_vec(),
        );
        pretty_assertions::assert_eq!(
            faux_scroll_input_bytes(PaneScrollDirection::Down, false),
            b"\x1b[B\x1b[B\x1b[B".to_vec(),
        );
    }

    #[test]
    fn test_faux_scroll_input_bytes_when_application_cursor_mode_is_enabled_uses_ss3_arrows() {
        pretty_assertions::assert_eq!(
            faux_scroll_input_bytes(PaneScrollDirection::Up, true),
            b"\x1bOA\x1bOA\x1bOA".to_vec(),
        );
        pretty_assertions::assert_eq!(
            faux_scroll_input_bytes(PaneScrollDirection::Down, true),
            b"\x1bOB\x1bOB\x1bOB".to_vec(),
        );
    }
}
