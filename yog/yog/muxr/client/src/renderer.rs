use std::io::Write;
use std::time::Instant;

use muxr_config::MuxrConfig;
use muxr_config::SelectionStyle;
use muxr_config::TabBarConfig;
use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::ClientMousePosition;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::PaneMouseMode;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::PaneScrollDirection;
use muxr_core::PaneScrollLineMove;
use muxr_core::RenderUpdate;
use muxr_core::TabId;
use rootcause::prelude::ResultExt;

use crate::copy_selection::SelectionChange;
use crate::copy_selection::SelectionClickOutcome;
use crate::copy_selection::SelectionClickTracker;
use crate::copy_selection::SelectionEdgeScrollPending;
use crate::copy_selection::SelectionEdgeScrollRequest;
use crate::copy_selection::SelectionEdgeScrollState;
use crate::copy_selection::SelectionInput;
use crate::copy_selection::SelectionRange;
use crate::copy_selection::SelectionState;
use crate::frame_buffer::ApplyOutcome;
use crate::frame_buffer::FrameBuffer;
use crate::frame_buffer::RenderFrameChanges;
use crate::frame_buffer::RenderFrameScope;
use crate::frame_buffer::SelectionHighlight;
use crate::frame_buffer::TerminalOrigin;
use crate::frame_buffer::TerminalRender;
use crate::frame_buffer::TerminalUpdateEncoder;
use crate::terminal::MouseAnyMotionCapture;
use crate::terminal::SynchronizedOutput;

const MAX_RETAINED_RENDER_TRANSACTION_BYTES: usize = 64 * 1_024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientRenderOutcome {
    Drawn,
    NeedsResync,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MouseCapture {
    region: PaneRegionSnapshot,
}

impl MouseCapture {
    fn retain_for_regions(self, regions: &PaneRegionsSnapshot) -> Option<Self> {
        self::region_for_pane_id(regions, *self.region.id())
            .cloned()
            .map(|region| Self { region })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TabBarDmg {
    Clean,
    Dirty,
}

pub struct ClientRenderer {
    any_motion_capture: MouseAnyMotionCapture,
    tab_bar_dmg: TabBarDmg,
    selection_style: SelectionStyle,
    tab_bar_config: TabBarConfig,
    clicks: SelectionClickTracker,
    frame_buffer: FrameBuffer,
    layout: LayoutSnapshot,
    mouse_capture: Option<MouseCapture>,
    pane_regions: PaneRegionsSnapshot,
    selection_edge_scroll: SelectionEdgeScrollState,
    selection: SelectionState,
    synchronized_output: SynchronizedOutput,
    terminal_encoder: TerminalUpdateEncoder,
    render_transaction: Vec<u8>,
}

impl ClientRenderer {
    pub fn new(config: &MuxrConfig, layout: LayoutSnapshot, pane_regions: PaneRegionsSnapshot) -> Self {
        Self::with_config_and_synchronized_output(
            config,
            layout,
            pane_regions,
            SynchronizedOutput::for_term(std::env::var("TERM").ok().as_deref()),
        )
    }

    #[cfg(test)]
    pub fn with_synchronized_output(
        layout: LayoutSnapshot,
        pane_regions: PaneRegionsSnapshot,
        synchronized_output: SynchronizedOutput,
    ) -> Self {
        let config = MuxrConfig::default();
        Self::with_config_and_synchronized_output(&config, layout, pane_regions, synchronized_output)
    }

    fn with_config_and_synchronized_output(
        config: &MuxrConfig,
        layout: LayoutSnapshot,
        pane_regions: PaneRegionsSnapshot,
        synchronized_output: SynchronizedOutput,
    ) -> Self {
        Self {
            any_motion_capture: MouseAnyMotionCapture::Disabled,
            tab_bar_dmg: TabBarDmg::Dirty,
            selection_style: config.selection,
            tab_bar_config: config.tab_bar,
            clicks: SelectionClickTracker::default(),
            frame_buffer: FrameBuffer::default(),
            layout,
            mouse_capture: None,
            pane_regions,
            selection_edge_scroll: SelectionEdgeScrollState::default(),
            selection: SelectionState::default(),
            synchronized_output,
            terminal_encoder: TerminalUpdateEncoder::default(),
            render_transaction: Vec::new(),
        }
    }

    pub fn apply_layout(&mut self, layout: LayoutSnapshot) {
        // Layout events precede their matching render baseline; defer tab bar writes so the user never sees new tab
        // state over an old pane frame.
        self.layout = layout;
        self.tab_bar_dmg = TabBarDmg::Dirty;
    }

    pub fn apply_sidebar_layout(&mut self, stdout: &mut impl Write, layout: LayoutSnapshot) -> rootcause::Result<()> {
        self.layout = layout;
        self.draw_sidebar(stdout)
    }

    pub fn tab_id_at_sidebar_row(&self, row: u16) -> Option<TabId> {
        crate::tab_bar::tab_id_at_row(&self.layout, row)
    }

    pub fn apply_pane_regions(
        &mut self,
        stdout: &mut impl Write,
        pane_regions: PaneRegionsSnapshot,
    ) -> rootcause::Result<()> {
        let previous_selection = self.selection.range().cloned();
        self.pane_regions = pane_regions;
        self.clicks.retain_for_regions(&self.pane_regions);
        self.mouse_capture = self
            .mouse_capture
            .take()
            .and_then(|capture| capture.retain_for_regions(&self.pane_regions));
        self.selection_edge_scroll.retain_for_regions(&self.pane_regions);
        let selection_change = self.selection.clear_if_regions_changed(&self.pane_regions);
        self.sync_mouse_capture(stdout)?;
        if selection_change == SelectionChange::Changed {
            let next_selection = self.selection.range().cloned();
            self.redraw_selection(stdout, previous_selection.as_ref(), next_selection.as_ref())?;
        }
        Ok(())
    }

    pub fn sync_mouse_capture(&mut self, stdout: &mut impl Write) -> rootcause::Result<()> {
        let next = if self
            .pane_regions
            .regions()
            .iter()
            .any(|region| region.mouse_mode() == PaneMouseMode::AnyMotion)
        {
            MouseAnyMotionCapture::Enabled
        } else {
            MouseAnyMotionCapture::Disabled
        };
        if self.any_motion_capture == next {
            return Ok(());
        }

        crate::terminal::set_mouse_any_motion_capture(stdout, next)?;
        self.any_motion_capture = next;
        Ok(())
    }

    pub fn apply_render(
        &mut self,
        stdout: &mut impl Write,
        update: RenderUpdate,
    ) -> rootcause::Result<ClientRenderOutcome> {
        match self.frame_buffer.apply(update)? {
            ApplyOutcome::Applied(changes) => {
                self.selection.refresh_visible_rows(&self.frame_buffer)?;
                self.draw(stdout, &changes)?;
                self.refresh_edge_drag_selection(stdout)?;
                self.selection_edge_scroll.clear_render_acknowledged_pending();
                Ok(ClientRenderOutcome::Drawn)
            }
            ApplyOutcome::NeedsResync => Ok(ClientRenderOutcome::NeedsResync),
        }
    }

    fn draw(&mut self, stdout: &mut impl Write, changes: &RenderFrameChanges) -> rootcause::Result<()> {
        let render_tab_bar = self.tab_bar_dmg == TabBarDmg::Dirty || changes.scope() == RenderFrameScope::Full;
        self.render_transaction.clear();
        let result = self.draw_transaction(stdout, changes, render_tab_bar);
        self.reset_render_transaction();
        if result.is_ok() {
            self.tab_bar_dmg = TabBarDmg::Clean;
        }
        result
    }

    fn draw_transaction(
        &mut self,
        stdout: &mut impl Write,
        changes: &RenderFrameChanges,
        render_tab_bar: bool,
    ) -> rootcause::Result<()> {
        crate::terminal::queue_synchronized_update_start(&mut self.render_transaction, self.synchronized_output)?;
        if changes.scope() == RenderFrameScope::Full {
            crate::frame_buffer::queue_full_redraw_start(&mut self.render_transaction)?;
        }
        if render_tab_bar {
            let rows = self.frame_buffer.size().map_or(0, muxr_core::TerminalSize::rows);
            crate::tab_bar::queue(&mut self.render_transaction, self.tab_bar_config, &self.layout, rows)?;
        }
        let selection = self.selection.range().map(|range| SelectionHighlight {
            background: self.selection_style.bg,
            range,
        });
        let render = TerminalRender {
            changes,
            frame_buffer: &self.frame_buffer,
            origin: TerminalOrigin {
                col: self.tab_bar_config.width,
                row: 0,
            },
            selection,
        };
        self.terminal_encoder.encode(&mut self.render_transaction, render)?;
        crate::terminal::queue_synchronized_update_end(&mut self.render_transaction, self.synchronized_output)?;
        stdout
            .write_all(&self.render_transaction)
            .context("failed to write muxr client render transaction")?;
        stdout
            .flush()
            .context("failed to flush muxr client render transaction")?;
        Ok(())
    }

    fn draw_sidebar(&mut self, stdout: &mut impl Write) -> rootcause::Result<()> {
        let Some(size) = self.frame_buffer.size() else {
            self.tab_bar_dmg = TabBarDmg::Dirty;
            return Ok(());
        };
        self.render_transaction.clear();
        let result = self.draw_sidebar_transaction(stdout, size.rows());
        self.reset_render_transaction();
        if result.is_ok() {
            self.tab_bar_dmg = TabBarDmg::Clean;
        }
        result
    }

    fn draw_sidebar_transaction(&mut self, stdout: &mut impl Write, rows: u16) -> rootcause::Result<()> {
        crate::terminal::queue_synchronized_update_start(&mut self.render_transaction, self.synchronized_output)?;
        crate::tab_bar::queue(&mut self.render_transaction, self.tab_bar_config, &self.layout, rows)?;
        // Sidebar-only redraws bypass pane rendering, so restore the real terminal cursor after tab-bar row moves.
        TerminalUpdateEncoder::encode_cursor(
            &mut self.render_transaction,
            &self.frame_buffer,
            TerminalOrigin {
                col: self.tab_bar_config.width,
                row: 0,
            },
        )?;
        crate::terminal::queue_synchronized_update_end(&mut self.render_transaction, self.synchronized_output)?;
        stdout
            .write_all(&self.render_transaction)
            .context("failed to write muxr client sidebar render transaction")?;
        stdout
            .flush()
            .context("failed to flush muxr client sidebar render transaction")?;
        Ok(())
    }

    fn reset_render_transaction(&mut self) {
        self.render_transaction.clear();
        if self.render_transaction.capacity() > MAX_RETAINED_RENDER_TRANSACTION_BYTES {
            self.render_transaction = Vec::new();
        }
    }

    pub fn apply_selection_input(&mut self, stdout: &mut impl Write, input: SelectionInput) -> rootcause::Result<()> {
        self.apply_selection_input_at(stdout, input, Instant::now())
    }

    pub fn apply_selection_input_at(
        &mut self,
        stdout: &mut impl Write,
        input: SelectionInput,
        now: Instant,
    ) -> rootcause::Result<()> {
        let previous_selection = self.selection.range().cloned();
        if matches!(input, SelectionInput::Start(_) | SelectionInput::End(_)) {
            self.selection_edge_scroll.clear();
        }
        let change = match input {
            SelectionInput::Start(position)
                if self
                    .clicks
                    .record_selection_start(position, &self.pane_regions, &self.frame_buffer, now)
                    == SelectionClickOutcome::Double =>
            {
                self.selection
                    .select_word(position, &self.pane_regions, &self.frame_buffer)?
            }
            SelectionInput::Start(position) => {
                self.selection
                    .apply(SelectionInput::Start(position), &self.pane_regions, &self.frame_buffer)?
            }
            SelectionInput::Update(position) => {
                self.selection
                    .apply(SelectionInput::Update(position), &self.pane_regions, &self.frame_buffer)?
            }
            SelectionInput::End(position) => {
                self.selection
                    .apply(SelectionInput::End(position), &self.pane_regions, &self.frame_buffer)?
            }
        };
        if change == SelectionChange::Changed {
            let next_selection = self.selection.range().cloned();
            self.redraw_selection(stdout, previous_selection.as_ref(), next_selection.as_ref())?;
        }
        Ok(())
    }

    pub fn mouse_request_for_event(&mut self, event: ClientMouseEvent) -> Option<ClientMouseEvent> {
        if crate::pane::scroll::MouseWheelEvent::from(event) == crate::pane::scroll::MouseWheelEvent::Wheel {
            return None;
        }

        if let Some(capture) = self.mouse_capture.as_ref() {
            let event = ClientMouseEvent {
                position: self::clamp_mouse_position_to_region(event.position, &capture.region),
                ..event
            };
            if event.phase == ClientMouseEventPhase::Release {
                self.mouse_capture = None;
            }
            return Some(event);
        }

        let region = self.pane_regions.pane_at(event.position)?;
        if region.mouse_mode() == PaneMouseMode::None {
            return None;
        }
        if MouseCaptureStart::from(event) == MouseCaptureStart::Start {
            self.mouse_capture = Some(MouseCapture { region: region.clone() });
        }
        Some(event)
    }

    pub fn copy_selection(&self) -> rootcause::Result<()> {
        let Some(text) = self.selection.selected_text() else {
            return Ok(());
        };
        crate::copy_selection::copy_to_clipboard(&text)
    }

    pub fn copy_selection_inline(&self) -> rootcause::Result<()> {
        let Some(text) = self.selection.selected_inline_text() else {
            return Ok(());
        };
        crate::copy_selection::copy_to_clipboard(&text)
    }

    pub fn set_selection_edge_drag(
        &mut self,
        position: ClientMousePosition,
        forced_direction: Option<PaneScrollDirection>,
    ) -> Option<SelectionEdgeScrollRequest> {
        let drag_region = self.selection.drag_region().cloned();
        self.selection_edge_scroll
            .set_edge_drag(position, forced_direction, drag_region.as_ref())
    }

    pub fn set_selection_outside_edge_drag(
        &mut self,
        position: ClientMousePosition,
    ) -> Option<SelectionEdgeScrollRequest> {
        let drag_region = self.selection.drag_region().cloned();
        self.selection_edge_scroll
            .set_outside_edge_drag(position, drag_region.as_ref())
    }

    fn refresh_edge_drag_selection(&mut self, stdout: &mut impl Write) -> rootcause::Result<()> {
        let Some(position) = self.selection_edge_scroll.drag_position(&self.pane_regions) else {
            self.selection_edge_scroll.clear();
            return Ok(());
        };
        // Edge-drag scrolling changes the viewport before the next mouse packet arrives; refresh the drag focus after
        // the scrolled frame renders so the selected range grows with the content under the held pointer.
        self.apply_selection_input(stdout, SelectionInput::Update(position))
    }

    pub fn selection_edge_scroll_request(&self) -> Option<SelectionEdgeScrollRequest> {
        self.selection_edge_scroll.scroll_request(&self.pane_regions)
    }

    pub fn apply_scroll_pane_line_result(
        &mut self,
        position: ClientMousePosition,
        direction: PaneScrollDirection,
        movement: PaneScrollLineMove,
    ) {
        self.selection_edge_scroll
            .apply_scroll_pane_line_result(position, direction, movement);
    }

    fn redraw_selection(
        &mut self,
        stdout: &mut impl Write,
        previous: Option<&SelectionRange>,
        next: Option<&SelectionRange>,
    ) -> rootcause::Result<()> {
        let rows = crate::copy_selection::changed_selection_rows(previous, next);
        let Some(changes) = self.frame_buffer.row_redraw_changes(&rows)? else {
            return Ok(());
        };
        self.draw(stdout, &changes)
    }

    pub const fn mouse_capture_state(&self) -> MouseCaptureState {
        match self.mouse_capture {
            Some(_) => MouseCaptureState::Captured,
            None => MouseCaptureState::None,
        }
    }

    pub const fn selection_edge_drag(&self) -> SelectionEdgeDrag {
        match self.selection_edge_scroll.active_state() {
            crate::copy_selection::SelectionEdgeScrollActive::Active => SelectionEdgeDrag::Active,
            crate::copy_selection::SelectionEdgeScrollActive::Inactive => SelectionEdgeDrag::Inactive,
        }
    }

    pub const fn mark_selection_edge_scroll_sent(&mut self, pending: SelectionEdgeScrollPending) {
        self.selection_edge_scroll.mark_sent(pending);
    }
}

fn region_for_pane_id(regions: &PaneRegionsSnapshot, pane_id: PaneId) -> Option<&PaneRegionSnapshot> {
    regions.regions().iter().find(|region| *region.id() == pane_id)
}

fn clamp_mouse_position_to_region(position: ClientMousePosition, region: &PaneRegionSnapshot) -> ClientMousePosition {
    ClientMousePosition {
        row: position
            .row
            .clamp(region.row(), self::last_region_row_saturating(region)),
        col: position
            .col
            .clamp(region.col(), self::last_region_col_saturating(region)),
    }
}

const fn last_region_col_saturating(region: &PaneRegionSnapshot) -> u16 {
    region.col().saturating_add(region.cols().saturating_sub(1))
}

const fn last_region_row_saturating(region: &PaneRegionSnapshot) -> u16 {
    region.row().saturating_add(region.rows().saturating_sub(1))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseCaptureState {
    Captured,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionEdgeDrag {
    Active,
    Inactive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MouseCaptureStart {
    Ignore,
    Start,
}

impl From<ClientMouseEvent> for MouseCaptureStart {
    fn from(event: ClientMouseEvent) -> Self {
        if event.phase == ClientMouseEventPhase::Press && event.button & (32 | 64) == 0 && event.button & 0b11 != 0b11 {
            Self::Start
        } else {
            Self::Ignore
        }
    }
}

#[cfg(test)]
pub mod test_helpers {
    use super::*;

    pub fn selected_text(renderer: &ClientRenderer) -> Option<String> {
        renderer.selection.selected_text()
    }

    pub fn selection_contains(renderer: &ClientRenderer, row: u16, col: u16) -> bool {
        renderer
            .selection
            .range()
            .is_some_and(|selection| selection.contains(row, col))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use muxr_core::ClientRequest;
    use muxr_core::PaneSnapshot;
    use muxr_core::TabId;
    use muxr_core::TabSnapshot;
    use muxr_core::TerminalSize;
    use rootcause::report;
    use test_that::prelude::*;

    use super::*;
    use crate::renderer::test_helpers;

    #[test]
    fn test_client_renderer_apply_layout_when_no_render_arrives_writes_nothing() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let output = CountingWriter::default();

        renderer.apply_layout(two_tab_layout()?);

        assert_that!(output.bytes, eq(Vec::<u8>::new()));
        assert_that!(output.flushes, eq(0));
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_sidebar_layout_when_frame_exists_flushes_only_sidebar() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let mut initial_output = CountingWriter::default();
        renderer.apply_render(
            &mut initial_output,
            muxr_core::RenderUpdate::Baseline(render_baseline()?),
        )?;
        let mut output = CountingWriter::default();

        renderer.apply_sidebar_layout(&mut output, two_tab_layout()?)?;

        let terminal_output = output.rendered_string()?;
        assert_that!(terminal_output, starts_with("\x1b[?2026h"));
        assert_that!(terminal_output, ends_with("\x1b[?2026l"));
        assert_that!(terminal_output, contains_substring("tab-1"));
        assert_that!(terminal_output, not(contains_substring("\x1b[2J")));
        let hide_index = terminal_output
            .find("\x1b[?25l")
            .ok_or_else(|| report!("expected cursor hide before sidebar redraw"))?;
        let tab_bar_index = terminal_output
            .find("tab-1")
            .ok_or_else(|| report!("expected tab bar text"))?;
        let cursor_restore_index = terminal_output
            .find("\x1b[1;26H\x1b[?25h")
            .ok_or_else(|| report!("expected pane cursor restore after sidebar redraw"))?;
        assert_that!(hide_index, lt(tab_bar_index));
        assert_that!(tab_bar_index, lt(cursor_restore_index));
        assert_that!(output.flushes, eq(1));
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_render_when_layout_is_dirty_flushes_one_complete_frame() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_layout(two_tab_layout()?);
        let mut output = CountingWriter::default();

        let outcome = renderer.apply_render(&mut output, muxr_core::RenderUpdate::Baseline(render_baseline()?))?;

        assert_that!(outcome, eq(ClientRenderOutcome::Drawn));
        assert_that!(output.flushes, eq(1));
        let terminal_output = output.rendered_string()?;
        assert_that!(terminal_output, starts_with("\x1b[?2026h"));
        assert_that!(terminal_output, ends_with("\x1b[?2026l"));
        let clear_index = terminal_output.find("\x1b[2J").unwrap_or(usize::MAX);
        let tab_bar_index = terminal_output.find("tab-1").unwrap_or(usize::MAX);
        let pane_index = terminal_output.find("ab").unwrap_or(usize::MAX);
        assert_that!(clear_index, lt(tab_bar_index));
        assert_that!(tab_bar_index, lt(pane_index));
        Ok(())
    }

    #[test]
    fn test_client_renderer_when_render_write_fails_reuses_clean_transaction_on_retry() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let mut failed_output = FailingWriter;

        assert_that!(
            renderer
                .apply_render(
                    &mut failed_output,
                    muxr_core::RenderUpdate::Baseline(render_baseline()?),
                )
                .is_err(),
            eq(true)
        );
        let mut output = CountingWriter::default();
        renderer.apply_render(&mut output, muxr_core::RenderUpdate::Baseline(render_baseline()?))?;

        let terminal_output = output.rendered_string()?;
        assert_that!(terminal_output.matches("\x1b[?2026h").count(), eq(1));
        assert_that!(terminal_output.matches("\x1b[?2026l").count(), eq(1));
        Ok(())
    }

    #[test]
    fn test_client_renderer_when_render_transaction_capacity_is_outlier_discards_it() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer
            .render_transaction
            .reserve(MAX_RETAINED_RENDER_TRANSACTION_BYTES + 1);

        renderer.reset_render_transaction();

        assert_that!(
            renderer.render_transaction.capacity(),
            le(MAX_RETAINED_RENDER_TRANSACTION_BYTES)
        );
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_render_when_resync_is_needed_does_not_flush() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let mut output = CountingWriter::default();

        let outcome = renderer.apply_render(&mut output, muxr_core::RenderUpdate::Diff(render_diff()?))?;

        assert_that!(outcome, eq(ClientRenderOutcome::NeedsResync));
        assert_that!(output.bytes, eq(Vec::<u8>::new()));
        assert_that!(output.flushes, eq(0));
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_pane_regions_when_any_motion_is_needed_enables_outer_capture() -> rootcause::Result<()>
    {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let mut output = CountingWriter::default();

        renderer.apply_pane_regions(&mut output, any_motion_pane_regions_snapshot()?)?;

        assert_that!(output.rendered_string()?, eq("\x1b[?1003h"));
        assert_that!(output.flushes, eq(1));
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_pane_regions_when_any_motion_is_no_longer_needed_disables_outer_capture()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            any_motion_pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let mut output = CountingWriter::default();

        renderer.sync_mouse_capture(&mut output)?;
        renderer.apply_pane_regions(&mut output, pane_regions_snapshot()?)?;

        assert_that!(
            output.rendered_string()?,
            eq("\x1b[?1003h\x1b[?1003l\x1b[?1000h\x1b[?1002h\x1b[?1006h")
        );
        assert_that!(output.flushes, eq(2));
        Ok(())
    }

    #[test]
    fn test_client_renderer_mouse_request_for_event_when_wheel_in_tracking_pane_returns_none() -> rootcause::Result<()>
    {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            any_motion_pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );

        assert_that!(
            renderer.mouse_request_for_event(ClientMouseEvent {
                button: 64,
                phase: ClientMouseEventPhase::Press,
                position: ClientMousePosition { row: 0, col: 0 },
            }),
            eq(None)
        );
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_selection_input_when_frame_exists_redraws_highlighted_selection()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let mut initial_output = CountingWriter::default();
        renderer.apply_render(
            &mut initial_output,
            muxr_core::RenderUpdate::Baseline(render_baseline()?),
        )?;
        let mut output = CountingWriter::default();

        renderer.apply_selection_input(
            &mut output,
            SelectionInput::Start(muxr_core::ClientMousePosition { row: 0, col: 0 }),
        )?;
        renderer.apply_selection_input(
            &mut output,
            SelectionInput::Update(muxr_core::ClientMousePosition { row: 0, col: 1 }),
        )?;

        assert_that!(test_helpers::selection_contains(&renderer, 0, 0), eq(true));
        assert_that!(test_helpers::selection_contains(&renderer, 0, 1), eq(true));
        let selection_output = output.rendered_string()?;
        assert_that!(selection_output, not(contains_substring("\x1b[7m")));
        assert_that!(output.flushes, eq(1));
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_pane_regions_when_selection_viewport_changes_redraws_selection_rows()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let mut initial_output = CountingWriter::default();
        renderer.apply_render(
            &mut initial_output,
            muxr_core::RenderUpdate::Baseline(render_baseline()?),
        )?;
        renderer.apply_selection_input(
            &mut initial_output,
            SelectionInput::Start(muxr_core::ClientMousePosition { row: 0, col: 0 }),
        )?;
        renderer.apply_selection_input(
            &mut initial_output,
            SelectionInput::End(muxr_core::ClientMousePosition { row: 0, col: 1 }),
        )?;
        let mut output = CountingWriter::default();

        renderer.apply_pane_regions(&mut output, pane_regions_snapshot_with_visible_top_row(1)?)?;

        let redrawn = output.rendered_string()?;
        assert_that!(redrawn, contains_substring("ab"));
        assert_that!(redrawn, not(contains_substring("\x1b[7m")));
        assert_that!(output.flushes, eq(1));
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_render_when_edge_drag_scrolls_extends_selection_after_viewport_moves()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            three_row_pane_regions_snapshot(9)?,
            SynchronizedOutput::Csi,
        );
        let mut output = CountingWriter::default();
        renderer.apply_render(
            &mut output,
            muxr_core::RenderUpdate::Baseline(three_row_render_baseline("aa", "bb", "cc")?),
        )?;
        renderer.apply_selection_input(
            &mut output,
            SelectionInput::Start(ClientMousePosition { row: 0, col: 0 }),
        )?;
        let scroll_request = renderer
            .set_selection_edge_drag(ClientMousePosition { row: 3, col: 1 }, None)
            .map(|request| request.into_parts().1);
        renderer.apply_selection_input(
            &mut output,
            SelectionInput::Update(ClientMousePosition { row: 3, col: 1 }),
        )?;

        assert_that!(
            scroll_request,
            eq(Some(ClientRequest::ScrollPaneLineAt {
                direction: PaneScrollDirection::Down,
                position: ClientMousePosition { row: 2, col: 1 },
            }))
        );

        renderer.apply_pane_regions(&mut output, three_row_pane_regions_snapshot(10)?)?;
        renderer.apply_render(
            &mut output,
            muxr_core::RenderUpdate::Baseline(three_row_render_baseline("bb", "cc", "dd")?),
        )?;

        assert_that!(
            test_helpers::selected_text(&renderer),
            eq(Some("aa\nbb\ncc\ndd".to_owned()))
        );
        assert_that!(test_helpers::selection_contains(&renderer, 2, 0), eq(true));
        Ok(())
    }

    #[rstest::rstest]
    #[case::top_edge(ClientMousePosition { row: 0, col: 1 }, PaneScrollDirection::Up)]
    #[case::bottom_edge(ClientMousePosition { row: 2, col: 1 }, PaneScrollDirection::Down)]
    fn test_client_renderer_set_selection_edge_drag_when_pointer_is_on_edge_row_requests_scroll(
        #[case] position: ClientMousePosition,
        #[case] direction: PaneScrollDirection,
    ) -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            three_row_pane_regions_snapshot(9)?,
            SynchronizedOutput::Csi,
        );
        let mut output = CountingWriter::default();
        renderer.apply_render(
            &mut output,
            muxr_core::RenderUpdate::Baseline(three_row_render_baseline("aa", "bb", "cc")?),
        )?;
        renderer.apply_selection_input(
            &mut output,
            SelectionInput::Start(ClientMousePosition { row: 1, col: 0 }),
        )?;

        let request = renderer
            .set_selection_edge_drag(position, None)
            .map(|request| request.into_parts().1);

        assert_that!(
            request,
            eq(Some(ClientRequest::ScrollPaneLineAt { position, direction }))
        );
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_scroll_pane_line_result_when_scroll_is_noop_clears_pending_request()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            three_row_pane_regions_snapshot(9)?,
            SynchronizedOutput::Csi,
        );
        let mut output = CountingWriter::default();
        renderer.apply_render(
            &mut output,
            muxr_core::RenderUpdate::Baseline(three_row_render_baseline("aa", "bb", "cc")?),
        )?;
        renderer.apply_selection_input(
            &mut output,
            SelectionInput::Start(ClientMousePosition { row: 1, col: 0 }),
        )?;
        let position = ClientMousePosition { row: 2, col: 1 };
        let direction = PaneScrollDirection::Down;
        let request = renderer
            .set_selection_edge_drag(position, None)
            .ok_or_else(|| report!("expected muxr edge scroll request"))?;
        let (pending, _) = request.into_parts();

        renderer.mark_selection_edge_scroll_sent(pending);
        assert_that!(renderer.selection_edge_scroll_request(), eq(None));
        renderer.apply_scroll_pane_line_result(position, direction, PaneScrollLineMove::Unchanged);

        let retry = renderer
            .selection_edge_scroll_request()
            .map(|request| request.into_parts().1);
        assert_that!(retry, eq(Some(ClientRequest::ScrollPaneLineAt { position, direction })));
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_scroll_pane_line_result_when_scroll_moves_waits_for_render_ack()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            three_row_pane_regions_snapshot(9)?,
            SynchronizedOutput::Csi,
        );
        let mut output = CountingWriter::default();
        renderer.apply_render(
            &mut output,
            muxr_core::RenderUpdate::Baseline(three_row_render_baseline("aa", "bb", "cc")?),
        )?;
        renderer.apply_selection_input(
            &mut output,
            SelectionInput::Start(ClientMousePosition { row: 1, col: 0 }),
        )?;
        let position = ClientMousePosition { row: 2, col: 1 };
        let direction = PaneScrollDirection::Down;
        let request = renderer
            .set_selection_edge_drag(position, None)
            .ok_or_else(|| report!("expected muxr edge scroll request"))?;
        let (pending, _) = request.into_parts();

        renderer.mark_selection_edge_scroll_sent(pending);
        renderer.apply_scroll_pane_line_result(position, direction, PaneScrollLineMove::Moved);

        assert_that!(renderer.selection_edge_scroll_request(), eq(None));
        renderer.apply_pane_regions(&mut output, three_row_pane_regions_snapshot(10)?)?;
        renderer.apply_render(
            &mut output,
            muxr_core::RenderUpdate::Baseline(three_row_render_baseline("bb", "cc", "dd")?),
        )?;
        let retry = renderer
            .selection_edge_scroll_request()
            .map(|request| request.into_parts().1);
        assert_that!(retry, eq(Some(ClientRequest::ScrollPaneLineAt { position, direction })));
        Ok(())
    }

    #[rstest::rstest]
    #[case::same_cell(4, 4)]
    #[case::same_word_different_cell(4, 6)]
    fn test_client_renderer_apply_selection_input_when_double_click_selects_visible_word(
        #[case] first_col: u16,
        #[case] second_col: u16,
    ) -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            word_pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let mut initial_output = CountingWriter::default();
        renderer.apply_render(
            &mut initial_output,
            muxr_core::RenderUpdate::Baseline(word_render_baseline()?),
        )?;
        let now = Instant::now();
        let first_position = ClientMousePosition { row: 0, col: first_col };
        let second_position = ClientMousePosition {
            row: 0,
            col: second_col,
        };
        let second_click_at = now
            .checked_add(Duration::from_millis(100))
            .ok_or_else(|| report!("muxr double-click selection test instant overflowed"))?;
        let mut output = CountingWriter::default();

        renderer.apply_selection_input_at(&mut output, SelectionInput::Start(first_position), now)?;
        renderer.apply_selection_input_at(&mut output, SelectionInput::End(first_position), now)?;
        renderer.apply_selection_input_at(&mut output, SelectionInput::Start(second_position), second_click_at)?;

        assert_that!(test_helpers::selected_text(&renderer), eq(Some("two".to_owned())));
        let selection_output = output.rendered_string()?;
        assert_that!(selection_output, not(contains_substring("\x1b[7m")));
        assert_that!(output.flushes, eq(1));
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_pane_regions_when_same_pane_remains_keeps_double_click() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            word_pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let mut initial_output = CountingWriter::default();
        renderer.apply_render(
            &mut initial_output,
            muxr_core::RenderUpdate::Baseline(word_render_baseline()?),
        )?;
        let now = Instant::now();
        let position = ClientMousePosition { row: 0, col: 4 };
        let second_click_at = now
            .checked_add(Duration::from_millis(100))
            .ok_or_else(|| report!("muxr retained double-click selection test instant overflowed"))?;
        let mut output = CountingWriter::default();

        renderer.apply_selection_input_at(&mut output, SelectionInput::Start(position), now)?;
        renderer.apply_selection_input_at(&mut output, SelectionInput::End(position), now)?;
        renderer.apply_pane_regions(&mut output, word_pane_regions_snapshot()?)?;
        renderer.apply_selection_input_at(&mut output, SelectionInput::Start(position), second_click_at)?;

        assert_that!(test_helpers::selected_text(&renderer), eq(Some("two".to_owned())));
        Ok(())
    }

    fn layout_snapshot() -> rootcause::Result<LayoutSnapshot> {
        let active_tab = TabId::new(1)?;
        let active_pane = PaneId::new(1)?;
        let pane = PaneSnapshot {
            tracked_process_state: muxr_core::TrackedProcessState::None,
            cwd: "/tmp/default".to_owned(),
            cmd_label: None,
            focus_seq: 1,
            id: active_pane,
            title: "shell".to_owned(),
        };
        let tab = TabSnapshot::new(active_tab, "default", active_pane, vec![pane])?;
        LayoutSnapshot::new(active_tab, vec![tab])
    }

    fn pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        self::pane_regions_snapshot_with_visible_top_row(0)
    }

    fn pane_regions_snapshot_with_visible_top_row(visible_top_row: u64) -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![muxr_core::PaneRegionSnapshot::new(
            muxr_core::PaneId::new(1)?,
            0,
            0,
            2,
            1,
            muxr_core::PaneMouseMode::None,
            visible_top_row,
        )?])
    }

    fn any_motion_pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![muxr_core::PaneRegionSnapshot::new(
            muxr_core::PaneId::new(1)?,
            0,
            0,
            2,
            1,
            muxr_core::PaneMouseMode::AnyMotion,
            0,
        )?])
    }

    fn word_pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![muxr_core::PaneRegionSnapshot::new(
            muxr_core::PaneId::new(1)?,
            0,
            0,
            7,
            1,
            muxr_core::PaneMouseMode::None,
            0,
        )?])
    }

    fn three_row_pane_regions_snapshot(visible_top_row: u64) -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![muxr_core::PaneRegionSnapshot::new(
            muxr_core::PaneId::new(1)?,
            0,
            0,
            2,
            3,
            muxr_core::PaneMouseMode::None,
            visible_top_row,
        )?])
    }

    fn two_tab_layout() -> rootcause::Result<LayoutSnapshot> {
        LayoutSnapshot::new(
            muxr_core::TabId::new(2)?,
            vec![
                muxr_core::TabSnapshot::new(
                    muxr_core::TabId::new(1)?,
                    "default",
                    muxr_core::PaneId::new(1)?,
                    vec![muxr_core::PaneSnapshot {
                        tracked_process_state: muxr_core::TrackedProcessState::None,
                        cwd: "/tmp/tab-1".to_owned(),
                        cmd_label: None,
                        focus_seq: 1,
                        id: muxr_core::PaneId::new(1)?,
                        title: "shell".to_owned(),
                    }],
                )?,
                muxr_core::TabSnapshot::new(
                    muxr_core::TabId::new(2)?,
                    "tab 2",
                    muxr_core::PaneId::new(2)?,
                    vec![muxr_core::PaneSnapshot {
                        tracked_process_state: muxr_core::TrackedProcessState::None,
                        cwd: "/tmp/tab-2".to_owned(),
                        cmd_label: None,
                        focus_seq: 1,
                        id: muxr_core::PaneId::new(2)?,
                        title: "shell".to_owned(),
                    }],
                )?,
            ],
        )
    }

    fn render_baseline() -> rootcause::Result<muxr_core::RenderBaseline> {
        muxr_core::RenderBaseline::new(
            1,
            TerminalSize::new(2, 1)?,
            muxr_core::RenderCursor {
                row: 0,
                col: 1,
                shape: muxr_core::RenderCursorShape::Default,
                visibility: muxr_core::RenderCursorVisibility::Visible,
            },
            vec![muxr_core::RenderRowSpan::new(
                0,
                0,
                vec![render_cell("a"), render_cell("b")],
            )?],
        )
    }

    fn word_render_baseline() -> rootcause::Result<muxr_core::RenderBaseline> {
        muxr_core::RenderBaseline::new(
            1,
            TerminalSize::new(7, 1)?,
            muxr_core::RenderCursor {
                row: 0,
                col: 1,
                shape: muxr_core::RenderCursorShape::Default,
                visibility: muxr_core::RenderCursorVisibility::Visible,
            },
            vec![muxr_core::RenderRowSpan::new(
                0,
                0,
                "one two".chars().map(|ch| render_cell(&ch.to_string())).collect(),
            )?],
        )
    }

    fn three_row_render_baseline(
        first: &str,
        second: &str,
        third: &str,
    ) -> rootcause::Result<muxr_core::RenderBaseline> {
        muxr_core::RenderBaseline::new(
            1,
            TerminalSize::new(2, 3)?,
            muxr_core::RenderCursor {
                row: 0,
                col: 1,
                shape: muxr_core::RenderCursorShape::Default,
                visibility: muxr_core::RenderCursorVisibility::Visible,
            },
            vec![
                muxr_core::RenderRowSpan::new(0, 0, first.chars().map(|ch| render_cell(&ch.to_string())).collect())?,
                muxr_core::RenderRowSpan::new(1, 0, second.chars().map(|ch| render_cell(&ch.to_string())).collect())?,
                muxr_core::RenderRowSpan::new(2, 0, third.chars().map(|ch| render_cell(&ch.to_string())).collect())?,
            ],
        )
    }

    fn render_diff() -> rootcause::Result<muxr_core::RenderDiff> {
        muxr_core::RenderDiff::new(
            1,
            2,
            TerminalSize::new(2, 1)?,
            muxr_core::RenderCursor {
                row: 0,
                col: 1,
                shape: muxr_core::RenderCursorShape::Default,
                visibility: muxr_core::RenderCursorVisibility::Visible,
            },
            vec![muxr_core::RenderRowSpan::new(0, 0, vec![render_cell("x")])?],
        )
    }

    fn render_cell(text: &str) -> muxr_core::RenderCell {
        muxr_core::RenderCell::narrow(text, muxr_core::RenderStyle::default())
    }

    #[derive(Default)]
    struct CountingWriter {
        bytes: Vec<u8>,
        flushes: usize,
    }

    impl CountingWriter {
        fn rendered_string(&self) -> rootcause::Result<String> {
            Ok(String::from_utf8(self.bytes.clone()).context("muxr client render test output was not utf8")?)
        }
    }

    impl Write for CountingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes = self.flushes.saturating_add(1);
            Ok(())
        }
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "injected muxr client write failure",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
