use std::time::Instant;

use muxr_core::ClientMousePosition;
use muxr_core::PaneId;
use muxr_core::TerminalSize;
use rootcause::report;

use crate::client::session::ClientSessionState;
use crate::pane::layout::PaneRegion;
use crate::pane::runtime::PaneRuntimes;
use crate::pane::tracked_process::PaneTrackedProcesses;
use crate::server::ServerConfig;
use crate::state::SessionLayout;
use crate::state::Tab;
use crate::terminal::TerminalFocusEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneFocusDirection {
    Down,
    Left,
    Right,
    Up,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneFocusRender {
    ResizePanesAndRender,
    SendLayoutAndBaseline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneFocusClientOutcome {
    Focused { render: PaneFocusRender },
    Unchanged,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PaneFocusChange {
    Changed,
    #[default]
    Unchanged,
}

impl SessionLayout {
    pub fn focus_pane_at(
        &mut self,
        size: &TerminalSize,
        position: ClientMousePosition,
    ) -> rootcause::Result<PaneFocusChange> {
        self.active_tab_mut()?.focus_pane_at(size, position)
    }

    pub fn focus_pane_direction(
        &mut self,
        size: &TerminalSize,
        direction: PaneFocusDirection,
    ) -> rootcause::Result<PaneFocusChange> {
        self.active_tab_mut()?.focus_pane_direction(size, direction)
    }
}

impl Tab {
    pub fn focus_pane_at(
        &mut self,
        size: &TerminalSize,
        position: ClientMousePosition,
    ) -> rootcause::Result<PaneFocusChange> {
        let Some(pane_id) = self.pane_at(size, position)? else {
            return Ok(PaneFocusChange::Unchanged);
        };

        self.focus_pane(pane_id)
    }

    pub fn focus_pane_direction(
        &mut self,
        size: &TerminalSize,
        direction: PaneFocusDirection,
    ) -> rootcause::Result<PaneFocusChange> {
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
            .filter(|region| PaneAdjacency::from_regions(region, active_region, direction) == PaneAdjacency::Adjacent)
            .max_by_key(|region| region.focus_seq)
            .map(|region| region.id)
        else {
            return Ok(PaneFocusChange::Unchanged);
        };

        self.focus_pane(next_pane_id)
    }

    pub fn focus_pane(&mut self, pane_id: PaneId) -> rootcause::Result<PaneFocusChange> {
        if self.active_pane == pane_id {
            let Some(pane) = self.pane_tree.pane_mut(pane_id) else {
                return Err(report!("muxr pane is missing from active tab").attach(format!("pane_id={pane_id}")));
            };
            return Ok(
                if pane.acknowledge_attention() == crate::pane::attention::PaneAttentionChange::Changed {
                    PaneFocusChange::Changed
                } else {
                    PaneFocusChange::Unchanged
                },
            );
        }

        let focus_seq = self.next_focus_seq()?;
        let Some(pane) = self.pane_tree.pane_mut(pane_id) else {
            return Err(report!("muxr pane is missing from active tab").attach(format!("pane_id={pane_id}")));
        };
        pane.set_focus_seq(focus_seq);
        let _acknowledged = pane.acknowledge_attention();
        self.active_pane = pane_id;
        Ok(PaneFocusChange::Changed)
    }
}

fn handle_focus_pane_cmd(
    direction: PaneFocusDirection,
    config: &ServerConfig,
    layout: &mut SessionLayout,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneFocusChange> {
    let focus_change = layout.focus_pane_direction(terminal_size, direction)?;
    if focus_change == PaneFocusChange::Changed {
        crate::state::persisted::write_metadata(&config.paths, layout)?;
    }
    Ok(focus_change)
}

fn handle_focus_pane_cmd_with_tracked_process_ack(
    direction: PaneFocusDirection,
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    pane_tracked_processes: &mut PaneTrackedProcesses,
    terminal_size: &TerminalSize,
    now: Instant,
) -> rootcause::Result<PaneFocusChange> {
    let focus_change = self::handle_focus_pane_cmd(direction, config, layout, terminal_size)?;
    if focus_change == PaneFocusChange::Changed {
        let _acknowledged = pane_tracked_processes.acknowledge_active_pane_attention(
            config.user_config.as_ref(),
            layout,
            runtimes,
            now,
        )?;
    }
    Ok(focus_change)
}

fn handle_focus_pane_at_request(
    position: ClientMousePosition,
    config: &ServerConfig,
    layout: &mut SessionLayout,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneFocusChange> {
    let focus_change = layout.focus_pane_at(terminal_size, position)?;
    if focus_change == PaneFocusChange::Changed {
        crate::state::persisted::write_metadata(&config.paths, layout)?;
    }
    Ok(focus_change)
}

fn handle_focus_pane_at_request_with_tracked_process_ack(
    position: ClientMousePosition,
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    pane_tracked_processes: &mut PaneTrackedProcesses,
    terminal_size: &TerminalSize,
    now: Instant,
) -> rootcause::Result<PaneFocusChange> {
    let focus_change = self::handle_focus_pane_at_request(position, config, layout, terminal_size)?;
    if focus_change == PaneFocusChange::Changed {
        let _acknowledged = pane_tracked_processes.acknowledge_active_pane_attention(
            config.user_config.as_ref(),
            layout,
            runtimes,
            now,
        )?;
    }
    Ok(focus_change)
}

pub fn handle_focus_pane_at_client_request(
    position: ClientMousePosition,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<PaneFocusClientOutcome> {
    // The scrollback editor layout is attached-client-local. Direct mouse focus must not mutate that temporary tree,
    // otherwise subsequent input can move away from the editor pane before restore.
    if state.scrollback_editor.is_some() {
        return Ok(PaneFocusClientOutcome::Unchanged);
    }
    if state.pane_fullscreen.visible_pane_id(state.layout)?.is_some() {
        return Ok(PaneFocusClientOutcome::Unchanged);
    }
    let previous_pane = state.layout.active_pane_id()?;
    if self::handle_focus_pane_at_request_with_tracked_process_ack(
        position,
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_tracked_processes,
        &state.terminal_size,
        Instant::now(),
    )? != PaneFocusChange::Changed
    {
        return Ok(PaneFocusClientOutcome::Unchanged);
    }
    self::write_active_pane_focus_events(previous_pane, state)?;
    Ok(PaneFocusClientOutcome::Focused {
        render: PaneFocusRender::SendLayoutAndBaseline,
    })
}

pub fn handle_focus_pane_cmd_client(
    direction: PaneFocusDirection,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<PaneFocusClientOutcome> {
    let previous_pane = state.layout.active_pane_id()?;
    if self::handle_focus_pane_cmd_with_tracked_process_ack(
        direction,
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_tracked_processes,
        &state.terminal_size,
        Instant::now(),
    )? != PaneFocusChange::Changed
    {
        return Ok(PaneFocusClientOutcome::Unchanged);
    }
    self::write_active_pane_focus_events(previous_pane, state)?;
    let render = if crate::pane::fullscreen::clear_active_tab_for_layout_mutation(state)
        == crate::pane::fullscreen::PaneFullscreenChange::Cleared
    {
        PaneFocusRender::ResizePanesAndRender
    } else {
        PaneFocusRender::SendLayoutAndBaseline
    };
    Ok(PaneFocusClientOutcome::Focused { render })
}

pub fn write_active_pane_focus_events(previous_pane: PaneId, state: &ClientSessionState<'_>) -> rootcause::Result<()> {
    let next_pane = state.layout.active_pane_id()?;
    self::write_pane_focus_transition(previous_pane, next_pane, state.runtimes)
}

fn write_pane_focus_transition(
    previous_pane: PaneId,
    next_pane: PaneId,
    runtimes: &PaneRuntimes,
) -> rootcause::Result<()> {
    // Focus reporting is a pane-application opt-in (`CSI ? 1004 h`). Close/reap may remove the old pane runtime
    // before the new pane receives focus, so skip missing runtimes while still notifying the surviving side.
    for (pane_id, event) in self::pane_focus_events_for_live_panes(previous_pane, next_pane, &runtimes.pane_ids()) {
        runtimes.handle(pane_id)?.write_focus_event(event)?;
    }
    Ok(())
}

fn pane_focus_events_for_live_panes(
    previous_pane: PaneId,
    next_pane: PaneId,
    live_panes: &[PaneId],
) -> Vec<(PaneId, TerminalFocusEvent)> {
    if previous_pane == next_pane {
        return Vec::new();
    }

    let mut events = Vec::with_capacity(2);
    if live_panes.contains(&previous_pane) {
        events.push((previous_pane, TerminalFocusEvent::Lost));
    }
    if live_panes.contains(&next_pane) {
        events.push((next_pane, TerminalFocusEvent::Gained));
    }
    events
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneAdjacency {
    Adjacent,
    Separate,
}

impl PaneAdjacency {
    fn from_regions(region: &PaneRegion, other: &PaneRegion, direction: PaneFocusDirection) -> Self {
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
        let horizontally_overlaps = PaneRangeOverlap::from_ranges(
            region_row,
            u32::from(region.area.size.rows),
            other_row,
            u32::from(other.area.size.rows),
        );
        let vertically_overlaps = PaneRangeOverlap::from_ranges(
            region_col,
            u32::from(region.area.size.cols),
            other_col,
            u32::from(other.area.size.cols),
        );

        let adjacent = match direction {
            PaneFocusDirection::Left => {
                Self::from_edges(region_end_col, other_col) == Self::Adjacent
                    && horizontally_overlaps == PaneRangeOverlap::Overlap
            }
            PaneFocusDirection::Right => {
                Self::from_edges(other_end_col, region_col) == Self::Adjacent
                    && horizontally_overlaps == PaneRangeOverlap::Overlap
            }
            PaneFocusDirection::Up => {
                Self::from_edges(region_end_row, other_row) == Self::Adjacent
                    && vertically_overlaps == PaneRangeOverlap::Overlap
            }
            PaneFocusDirection::Down => {
                Self::from_edges(other_end_row, region_row) == Self::Adjacent
                    && vertically_overlaps == PaneRangeOverlap::Overlap
            }
        };
        if adjacent { Self::Adjacent } else { Self::Separate }
    }

    fn from_edges(edge: u32, start: u32) -> Self {
        if edge == start || edge.checked_add(1) == Some(start) {
            Self::Adjacent
        } else {
            Self::Separate
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneRangeOverlap {
    Overlap,
    Separate,
}

impl PaneRangeOverlap {
    const fn from_ranges(first_start: u32, first_len: u32, second_start: u32, second_len: u32) -> Self {
        let first_end = first_start.saturating_add(first_len);
        let second_end = second_start.saturating_add(second_len);

        if first_start < second_end && second_start < first_end {
            Self::Overlap
        } else {
            Self::Separate
        }
    }
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;

    use super::*;
    use crate::pane::split::PaneSplitAxis;
    use crate::state::test_helpers as state_test_helpers;

    #[rstest::rstest]
    #[case::first_pane(ClientMousePosition { row: 0, col: 0 }, "pane-1", PaneFocusChange::Changed)]
    #[case::border(ClientMousePosition { row: 0, col: 40 }, "pane-2", PaneFocusChange::Unchanged)]
    #[case::second_pane(ClientMousePosition { row: 0, col: 41 }, "pane-2", PaneFocusChange::Unchanged)]
    fn test_layout_focus_pane_at_when_mouse_position_arrives_updates_active_pane(
        #[case] position: ClientMousePosition,
        #[case] expected_active_pane: &str,
        #[case] expected_change: PaneFocusChange,
    ) -> rootcause::Result<()> {
        let mut layout = state_test_helpers::layout("work")?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        state_test_helpers::force_balanced_test_split_ratio(&mut layout)?;

        pretty_assertions::assert_eq!(
            layout.focus_pane_at(&TerminalSize::new(80, 24)?, position)?,
            expected_change,
        );
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), expected_active_pane);
        Ok(())
    }

    #[test]
    fn test_pane_focus_events_for_live_panes_when_runtime_sets_vary_returns_focus_transition() -> rootcause::Result<()>
    {
        let previous_pane = PaneId::new(1)?;
        let next_pane = PaneId::new(2)?;

        for (previous_pane, next_pane, live_panes, expected) in [
            (
                previous_pane,
                next_pane,
                vec![previous_pane, next_pane],
                vec![
                    (previous_pane, TerminalFocusEvent::Lost),
                    (next_pane, TerminalFocusEvent::Gained),
                ],
            ),
            (previous_pane, previous_pane, vec![previous_pane], Vec::new()),
            (
                previous_pane,
                next_pane,
                vec![next_pane],
                vec![(next_pane, TerminalFocusEvent::Gained)],
            ),
            (
                previous_pane,
                next_pane,
                vec![previous_pane],
                vec![(previous_pane, TerminalFocusEvent::Lost)],
            ),
            (previous_pane, next_pane, Vec::new(), Vec::new()),
        ] {
            pretty_assertions::assert_eq!(
                self::pane_focus_events_for_live_panes(previous_pane, next_pane, &live_panes),
                expected,
            );
        }
        Ok(())
    }

    #[rstest::rstest]
    #[case::first_pane(ClientMousePosition { row: 0, col: 0 }, Some("pane-1"))]
    #[case::border(ClientMousePosition { row: 0, col: 40 }, None)]
    #[case::second_pane(ClientMousePosition { row: 0, col: 41 }, Some("pane-2"))]
    fn test_tab_pane_at_when_mouse_position_arrives_returns_pane_without_focus_change(
        #[case] position: ClientMousePosition,
        #[case] expected_pane: Option<&str>,
    ) -> rootcause::Result<()> {
        let mut layout = state_test_helpers::layout("work")?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        state_test_helpers::force_balanced_test_split_ratio(&mut layout)?;

        let pane_id = layout.active_tab()?.pane_at(&TerminalSize::new(80, 24)?, position)?;

        pretty_assertions::assert_eq!(pane_id.map(|id| id.to_string()), expected_pane.map(str::to_owned));
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-2");
        Ok(())
    }

    #[rstest::rstest]
    #[case::left(PaneFocusDirection::Left, "pane-1", PaneFocusChange::Changed)]
    #[case::right_edge(PaneFocusDirection::Right, "pane-2", PaneFocusChange::Unchanged)]
    #[case::up_edge(PaneFocusDirection::Up, "pane-2", PaneFocusChange::Unchanged)]
    #[case::down_edge(PaneFocusDirection::Down, "pane-2", PaneFocusChange::Unchanged)]
    fn test_layout_focus_pane_direction_when_adjacent_pane_exists_updates_active_pane(
        #[case] direction: PaneFocusDirection,
        #[case] expected_active_pane: &str,
        #[case] expected_change: PaneFocusChange,
    ) -> rootcause::Result<()> {
        let mut layout = state_test_helpers::layout("work")?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;

        pretty_assertions::assert_eq!(
            layout.focus_pane_direction(&TerminalSize::new(80, 24)?, direction)?,
            expected_change,
        );
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), expected_active_pane);
        Ok(())
    }

    #[test]
    fn test_layout_focus_pane_direction_when_multiple_adjacent_panes_exist_uses_recent_focus() -> rootcause::Result<()>
    {
        let mut layout = state_test_helpers::layout("work")?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 3),
            PaneSplitAxis::Horizontal,
        )?;

        pretty_assertions::assert_eq!(
            layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Up)?,
            PaneFocusChange::Changed,
        );
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-2");
        pretty_assertions::assert_eq!(
            layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Left)?,
            PaneFocusChange::Changed,
        );
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-1");

        pretty_assertions::assert_eq!(
            layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Right)?,
            PaneFocusChange::Changed,
        );

        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-2");
        Ok(())
    }
}
