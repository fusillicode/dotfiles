use std::sync::Mutex;

use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::PaneRegionSnapshot;
use muxr_core::TerminalSize;
use rootcause::report;

use crate::server::PaneRuntimes;
use crate::server::ServerConfig;
use crate::state::SessionLayout;
use crate::state::Tab;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneFocusDirection {
    Down,
    Left,
    Right,
    Up,
}

impl SessionLayout {
    pub fn focus_pane_at(&mut self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<bool> {
        self.active_tab_mut()?.focus_pane_at(size, position)
    }

    pub fn focus_pane_direction(
        &mut self,
        size: &TerminalSize,
        direction: PaneFocusDirection,
    ) -> rootcause::Result<bool> {
        self.active_tab_mut()?.focus_pane_direction(size, direction)
    }
}

impl Tab {
    pub fn focus_pane_at(&mut self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<bool> {
        let Some(pane_id) = self.pane_at(size, position)? else {
            return Ok(false);
        };

        self.focus_pane(pane_id)
    }

    pub fn focus_pane_direction(
        &mut self,
        size: &TerminalSize,
        direction: PaneFocusDirection,
    ) -> rootcause::Result<bool> {
        let layout = self.pane_layout(size)?;
        let active_region = layout
            .regions()
            .iter()
            .find(|region| region.id() == &self.active_pane)
            .ok_or_else(|| {
                report!("muxr active pane is missing from active tab layout")
                    .attach(format!("active_pane={}", self.active_pane))
            })?;
        let Some(next_pane_id) = layout
            .regions()
            .iter()
            .filter(|region| region.id() != active_region.id())
            .filter(|region| region.is_adjacent_to(active_region, direction))
            .max_by_key(|region| region.focus_seq())
            .map(|region| region.id().clone())
        else {
            return Ok(false);
        };

        self.focus_pane(next_pane_id)
    }

    pub fn focus_pane(&mut self, pane_id: PaneId) -> rootcause::Result<bool> {
        if self.active_pane == pane_id {
            return Ok(false);
        }

        let focus_seq = self.next_focus_seq()?;
        let Some(pane) = self.pane_tree.pane_mut(&pane_id) else {
            return Err(report!("muxr pane is missing from active tab").attach(format!("pane_id={pane_id}")));
        };
        pane.set_focus_seq(focus_seq);
        self.active_pane = pane_id;
        Ok(true)
    }
}

pub fn handle_focus_pane_command(
    direction: PaneFocusDirection,
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<bool> {
    let mut layout = crate::server::lock_mutex(layout, "layout")?;
    let focused = layout.focus_pane_direction(terminal_size, direction)?;
    if focused {
        crate::state::persisted::write_metadata(&config.paths, &layout)?;
    }
    drop(layout);
    Ok(focused)
}

pub fn handle_focus_pane_at_request(
    position: ClientMousePosition,
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<bool> {
    let mut layout = crate::server::lock_mutex(layout, "layout")?;
    let focused = layout.focus_pane_at(terminal_size, position)?;
    if focused {
        crate::state::persisted::write_metadata(&config.paths, &layout)?;
    }
    drop(layout);
    Ok(focused)
}

pub fn mouse_event_focuses_pane(event: ClientMouseEvent) -> bool {
    event.phase() == ClientMouseEventPhase::Press && event.button() & (32 | 64) == 0 && event.button() & 0b11 != 0b11
}

pub fn mouse_event_region(
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
    position: ClientMousePosition,
) -> rootcause::Result<Option<PaneRegionSnapshot>> {
    let region = {
        let layout = crate::server::lock_mutex(layout, "layout")?;
        let region = layout
            .pane_regions(terminal_size)?
            .into_iter()
            .find(|region| region.contains(position.row, position.col));
        drop(layout);
        let Some(region) = region else {
            return Ok(None);
        };
        region
    };
    let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
    let handle = runtimes.handle(region.id())?;
    let mouse_mode = handle.mouse_mode()?;
    let visible_top_row = handle.visible_top_row()?;
    drop(runtimes);
    Ok(Some(PaneRegionSnapshot::new(
        region.id().clone(),
        region.col(),
        region.row(),
        region.cols(),
        region.rows(),
        mouse_mode,
        visible_top_row,
    )?))
}
