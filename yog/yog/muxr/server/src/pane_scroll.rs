use std::sync::Mutex;

use muxr_core::ClientMousePosition;
use muxr_core::PaneScrollDirection;
use muxr_core::TerminalSize;

use crate::pane_runtime::PaneRuntimes;
use crate::state::SessionLayout;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneScrollAmount {
    Line,
    Wheel,
}

pub fn handle_scroll_pane_line_at_request(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<bool> {
    self::handle_scroll_pane_at_request(
        position,
        direction,
        PaneScrollAmount::Line,
        layout,
        runtimes,
        terminal_size,
    )
}

pub fn handle_scroll_pane_at_request(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    amount: PaneScrollAmount,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<bool> {
    let pane_id = {
        let layout = crate::server::lock_mutex(layout, "layout")?;
        let pane_id = layout.pane_at(terminal_size, position)?;
        drop(layout);
        let Some(pane_id) = pane_id else {
            return Ok(false);
        };
        pane_id
    };

    let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
    match amount {
        PaneScrollAmount::Line => runtimes.handle(pane_id)?.scroll_one_line(direction),
        PaneScrollAmount::Wheel => runtimes.handle(pane_id)?.scroll(direction),
    }
}
