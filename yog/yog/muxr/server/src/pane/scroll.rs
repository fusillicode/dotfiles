use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::PaneScrollDirection;
use muxr_core::ServerEvent;

use crate::client::session::ClientSessionState;
use crate::pane::runtime::PaneRuntimes;
use crate::terminal::TerminalCursorKeyMode;

const FAUX_SCROLL_LINES_PER_WHEEL_EVENT: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneScrollAmount {
    Line,
    Wheel,
}

pub struct PaneScrollLineRequestOutcome {
    pub event: ServerEvent,
    pub render_dirty: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneScrollWheelRequestOutcome {
    pub render_dirty: bool,
    pub sync_render_deadline: bool,
}

const fn scroll_pane_line_result(
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
        // Edge-drag autoscroll can outpace render IO; keep viewport changes coalesced on the render deadline.
        render_dirty: scrolled,
    }
}

pub fn handle_scroll_pane_line_client_request(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    state: &ClientSessionState<'_>,
) -> rootcause::Result<PaneScrollLineRequestOutcome> {
    let scrolled = if let Some(pane_id) = crate::screen_render::visible_pane_id_at_position(state, position)? {
        self::scroll_pane(pane_id, direction, PaneScrollAmount::Line, state.runtimes)?
    } else {
        false
    };
    Ok(self::scroll_pane_line_result(position, direction, scrolled))
}

pub fn handle_scroll_pane_wheel_client_request(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    state: &ClientSessionState<'_>,
) -> rootcause::Result<PaneScrollWheelRequestOutcome> {
    let Some(pane_id) = crate::screen_render::visible_pane_id_at_position(state, position)? else {
        return Ok(PaneScrollWheelRequestOutcome {
            render_dirty: false,
            sync_render_deadline: false,
        });
    };
    if !self::scroll_pane(pane_id, direction, PaneScrollAmount::Wheel, state.runtimes)? {
        return Ok(PaneScrollWheelRequestOutcome {
            render_dirty: false,
            sync_render_deadline: false,
        });
    }
    // Wheel input can arrive much faster than render IO; mark dirty and let the render deadline coalesce.
    Ok(PaneScrollWheelRequestOutcome {
        render_dirty: true,
        sync_render_deadline: true,
    })
}

fn scroll_pane(
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

pub fn faux_scroll_input_bytes(direction: PaneScrollDirection, cursor_key_mode: TerminalCursorKeyMode) -> Vec<u8> {
    let sequence = self::faux_scroll_sequence(direction, cursor_key_mode);
    let mut bytes = Vec::with_capacity(sequence.len().saturating_mul(FAUX_SCROLL_LINES_PER_WHEEL_EVENT));
    for _ in 0..FAUX_SCROLL_LINES_PER_WHEEL_EVENT {
        bytes.extend_from_slice(sequence);
    }
    bytes
}

const fn faux_scroll_sequence(direction: PaneScrollDirection, cursor_key_mode: TerminalCursorKeyMode) -> &'static [u8] {
    match (direction, cursor_key_mode) {
        (PaneScrollDirection::Up, TerminalCursorKeyMode::Normal) => b"\x1b[A",
        (PaneScrollDirection::Down, TerminalCursorKeyMode::Normal) => b"\x1b[B",
        (PaneScrollDirection::Up, TerminalCursorKeyMode::Application) => b"\x1bOA",
        (PaneScrollDirection::Down, TerminalCursorKeyMode::Application) => b"\x1bOB",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_faux_scroll_input_bytes_when_application_cursor_mode_is_disabled_uses_csi_arrows() {
        pretty_assertions::assert_eq!(
            faux_scroll_input_bytes(PaneScrollDirection::Up, TerminalCursorKeyMode::Normal),
            b"\x1b[A\x1b[A\x1b[A".to_vec(),
        );
        pretty_assertions::assert_eq!(
            faux_scroll_input_bytes(PaneScrollDirection::Down, TerminalCursorKeyMode::Normal),
            b"\x1b[B\x1b[B\x1b[B".to_vec(),
        );
    }

    #[test]
    fn test_faux_scroll_input_bytes_when_application_cursor_mode_is_enabled_uses_ss3_arrows() {
        pretty_assertions::assert_eq!(
            faux_scroll_input_bytes(PaneScrollDirection::Up, TerminalCursorKeyMode::Application),
            b"\x1bOA\x1bOA\x1bOA".to_vec(),
        );
        pretty_assertions::assert_eq!(
            faux_scroll_input_bytes(PaneScrollDirection::Down, TerminalCursorKeyMode::Application),
            b"\x1bOB\x1bOB\x1bOB".to_vec(),
        );
    }
}
