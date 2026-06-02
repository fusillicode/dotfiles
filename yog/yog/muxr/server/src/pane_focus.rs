use std::sync::Mutex;

use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::PaneRegionSnapshot;
use muxr_core::TerminalSize;
use rootcause::report;

use crate::pane_layout::PaneRegion;
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
            .find(|region| region.id == self.active_pane)
            .ok_or_else(|| {
                report!("muxr active pane is missing from active tab layout")
                    .attach(format!("active_pane={}", self.active_pane))
            })?;
        let Some(next_pane_id) = layout
            .regions()
            .iter()
            .filter(|region| region.id != active_region.id)
            .filter(|region| self::pane_regions_are_adjacent(region, active_region, direction))
            .max_by_key(|region| region.focus_seq)
            .map(|region| region.id.clone())
        else {
            return Ok(false);
        };

        self.focus_pane(next_pane_id)
    }

    pub fn focus_pane(&mut self, pane_id: PaneId) -> rootcause::Result<bool> {
        if self.active_pane == pane_id {
            let Some(pane) = self.pane_tree.pane_mut(&pane_id) else {
                return Err(report!("muxr pane is missing from active tab").attach(format!("pane_id={pane_id}")));
            };
            return Ok(pane.clear_attention());
        }

        let focus_seq = self.next_focus_seq()?;
        let Some(pane) = self.pane_tree.pane_mut(&pane_id) else {
            return Err(report!("muxr pane is missing from active tab").attach(format!("pane_id={pane_id}")));
        };
        pane.set_focus_seq(focus_seq);
        let _cleared = pane.clear_attention();
        self.active_pane = pane_id;
        Ok(true)
    }
}

pub fn handle_focus_pane_cmd(
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
    event.phase == ClientMouseEventPhase::Press && event.button & (32 | 64) == 0 && event.button & 0b11 != 0b11
}

fn pane_regions_are_adjacent(region: &PaneRegion, other: &PaneRegion, direction: PaneFocusDirection) -> bool {
    // Muxr pane regions exclude the separator cell, so visible neighbors have a one-cell gap where Zellij's
    // frame-inclusive pane geometry uses exact edge equality.
    let region_col = u32::from(region.area.origin.col);
    let region_row = u32::from(region.area.origin.row);
    let region_end_col = region.area.end_col_exclusive();
    let region_end_row = region.area.end_row_exclusive();
    let other_col = u32::from(other.area.origin.col);
    let other_row = u32::from(other.area.origin.row);
    let other_end_col = other.area.end_col_exclusive();
    let other_end_row = other.area.end_row_exclusive();
    let horizontally_overlaps = self::ranges_overlap(
        region_row,
        u32::from(region.area.size.rows),
        other_row,
        u32::from(other.area.size.rows),
    );
    let vertically_overlaps = self::ranges_overlap(
        region_col,
        u32::from(region.area.size.cols),
        other_col,
        u32::from(other.area.size.cols),
    );

    match direction {
        PaneFocusDirection::Left => self::edges_are_adjacent(region_end_col, other_col) && horizontally_overlaps,
        PaneFocusDirection::Right => self::edges_are_adjacent(other_end_col, region_col) && horizontally_overlaps,
        PaneFocusDirection::Up => self::edges_are_adjacent(region_end_row, other_row) && vertically_overlaps,
        PaneFocusDirection::Down => self::edges_are_adjacent(other_end_row, region_row) && vertically_overlaps,
    }
}

fn edges_are_adjacent(edge: u32, start: u32) -> bool {
    edge == start || edge.checked_add(1) == Some(start)
}

const fn ranges_overlap(first_start: u32, first_len: u32, second_start: u32, second_len: u32) -> bool {
    let first_end = first_start.saturating_add(first_len);
    let second_end = second_start.saturating_add(second_len);

    first_start < second_end && second_start < first_end
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
            .find(|region| region.contains(position.into()));
        drop(layout);
        let Some(region) = region else {
            return Ok(None);
        };
        region
    };
    let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
    let handle = runtimes.handle(&region.id)?;
    let mouse_mode = handle.mouse_mode()?;
    let visible_top_row = handle.visible_top_row()?;
    drop(runtimes);
    Ok(Some(PaneRegionSnapshot::new(
        region.id,
        region.area.origin.col,
        region.area.origin.row,
        region.area.size.cols,
        region.area.size.rows,
        mouse_mode,
        visible_top_row,
    )?))
}
