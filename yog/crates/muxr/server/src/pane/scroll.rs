use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::PaneScrollDirection;
use muxr_core::PaneScrollLineMove;
use muxr_core::ServerEvent;

use crate::client::session::ClientSessionState;
use crate::pane::runtime::PaneRuntimes;
use crate::render_state::PaneRenderSignal;
use crate::terminal::TerminalCursorKeyMode;

const FAUX_SCROLL_LINES_PER_WHEEL_EVENT: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneScrollAmount {
    Line,
    Wheel,
}

pub struct PaneScrollLineRequestOutcome {
    pub event: ServerEvent,
    pub render_signal: PaneRenderSignal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneScrollWheelRequestOutcome {
    pub render_signal: PaneRenderSignal,
}

fn scroll_pane_line_result(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    movement: PaneScrollLineMove,
    pane_id: Option<PaneId>,
) -> PaneScrollLineRequestOutcome {
    PaneScrollLineRequestOutcome {
        event: ServerEvent::ScrollPaneLineResult {
            position,
            direction,
            movement,
        },
        // Edge-drag autoscroll can outpace render IO; keep viewport changes coalesced on the render deadline.
        render_signal: match movement {
            PaneScrollLineMove::Moved => pane_id.map_or(
                PaneRenderSignal::DirtyAndDeadline(crate::render_state::ClientRenderDmg::Full),
                |pane_id| {
                    PaneRenderSignal::DirtyAndDeadline(crate::render_state::ClientRenderDmg::region_changed(pane_id))
                },
            ),
            PaneScrollLineMove::Unchanged => PaneRenderSignal::DeadlineOnly,
        },
    }
}

pub fn handle_scroll_pane_line_client_request(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    state: &ClientSessionState<'_>,
) -> rootcause::Result<PaneScrollLineRequestOutcome> {
    let pane_id = crate::screen_render::visible_pane_id_at_position(state, position)?;
    let movement = if let Some(pane_id) = pane_id {
        match self::scroll_pane(pane_id, direction, PaneScrollAmount::Line, state.runtimes)? {
            crate::terminal::TerminalScrollMove::Moved => PaneScrollLineMove::Moved,
            crate::terminal::TerminalScrollMove::Unchanged => PaneScrollLineMove::Unchanged,
        }
    } else {
        PaneScrollLineMove::Unchanged
    };
    Ok(self::scroll_pane_line_result(position, direction, movement, pane_id))
}

pub fn handle_scroll_pane_wheel_client_request(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    state: &ClientSessionState<'_>,
) -> rootcause::Result<PaneScrollWheelRequestOutcome> {
    let Some(pane_id) = crate::screen_render::visible_pane_id_at_position(state, position)? else {
        return Ok(PaneScrollWheelRequestOutcome {
            render_signal: PaneRenderSignal::Unchanged,
        });
    };
    if self::scroll_pane(pane_id, direction, PaneScrollAmount::Wheel, state.runtimes)?
        != crate::terminal::TerminalScrollMove::Moved
    {
        return Ok(PaneScrollWheelRequestOutcome {
            render_signal: PaneRenderSignal::Unchanged,
        });
    }
    // Wheel input can arrive much faster than render IO; mark dirty and let the render deadline coalesce.
    Ok(PaneScrollWheelRequestOutcome {
        render_signal: PaneRenderSignal::DirtyAndDeadline(crate::render_state::ClientRenderDmg::region_changed(
            pane_id,
        )),
    })
}

fn scroll_pane(
    pane_id: PaneId,
    direction: PaneScrollDirection,
    amount: PaneScrollAmount,
    runtimes: &PaneRuntimes,
) -> rootcause::Result<crate::terminal::TerminalScrollMove> {
    let handle = runtimes.handle(pane_id)?;
    Ok(match amount {
        PaneScrollAmount::Line => handle.scroll_one_line(direction),
        PaneScrollAmount::Wheel => handle.scroll(direction),
    })
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
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_faux_scroll_input_bytes_when_application_cursor_mode_is_disabled_uses_csi_arrows() {
        assert_that!(
            faux_scroll_input_bytes(PaneScrollDirection::Up, TerminalCursorKeyMode::Normal),
            eq(b"\x1b[A\x1b[A\x1b[A".to_vec())
        );
        assert_that!(
            faux_scroll_input_bytes(PaneScrollDirection::Down, TerminalCursorKeyMode::Normal),
            eq(b"\x1b[B\x1b[B\x1b[B".to_vec())
        );
    }

    #[test]
    fn test_faux_scroll_input_bytes_when_application_cursor_mode_is_enabled_uses_ss3_arrows() {
        assert_that!(
            faux_scroll_input_bytes(PaneScrollDirection::Up, TerminalCursorKeyMode::Application),
            eq(b"\x1bOA\x1bOA\x1bOA".to_vec())
        );
        assert_that!(
            faux_scroll_input_bytes(PaneScrollDirection::Down, TerminalCursorKeyMode::Application),
            eq(b"\x1bOB\x1bOB\x1bOB".to_vec())
        );
    }
}
