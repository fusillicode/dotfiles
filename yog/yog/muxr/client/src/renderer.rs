use std::collections::BTreeSet;
use std::time::Instant;

use muxr_config::MuxrConfig;
use muxr_config::SelectionStyle;
use muxr_config::TabBarConfig;
use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::ClientMousePosition;
use muxr_core::ClientRequest;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::PaneMouseMode;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::PaneScrollDirection;
use muxr_core::PaneScrollLineMove;
use muxr_core::RenderUpdate;
use muxr_core::TabId;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileOpenCapture {
    Armed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileOpenRelease {
    Consumed,
    NotCaptured,
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
    file_open_capture: Option<FileOpenCapture>,
    pane_regions: PaneRegionsSnapshot,
    selection_edge_scroll: SelectionEdgeScrollState,
    selection: SelectionState,
    synchronized_output: SynchronizedOutput,
    terminal_encoder: TerminalUpdateEncoder,
    render_transaction: Vec<u8>,
    render_generation: u64,
    edge_scroll_render_generation: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientPresentationSnapshot {
    any_motion_capture: MouseAnyMotionCapture,
    edge_scroll_pending: Option<crate::copy_selection::SelectionEdgeScrollPending>,
    frame_buffer: FrameBuffer,
    layout: LayoutSnapshot,
    edge_scroll_render_generation: Option<u64>,
    selection: Option<SelectionRange>,
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
            file_open_capture: None,
            pane_regions,
            selection_edge_scroll: SelectionEdgeScrollState::default(),
            selection: SelectionState::default(),
            synchronized_output,
            terminal_encoder: TerminalUpdateEncoder::default(),
            render_transaction: Vec::new(),
            render_generation: 0,
            edge_scroll_render_generation: None,
        }
    }

    pub fn apply_layout(&mut self, layout: LayoutSnapshot) {
        // Layout events precede their matching render baseline; defer tab bar writes so the user never sees new tab
        // state over an old pane frame.
        self.layout = layout;
        self.tab_bar_dmg = TabBarDmg::Dirty;
    }

    pub fn apply_sidebar_layout_logical(&mut self, layout: LayoutSnapshot) {
        self.layout = layout;
    }

    pub fn tab_id_at_sidebar_row(&self, row: u16) -> Option<TabId> {
        crate::tab_bar::tab_id_at_row(&self.layout, row)
    }

    pub(crate) fn tab_focus_request_for_sidebar_click(&self, event: ClientMouseEvent) -> Option<ClientRequest> {
        if event.phase != ClientMouseEventPhase::Press || event.button != 0 {
            return None;
        }
        self.tab_id_at_sidebar_row(event.position.row)
            .map(ClientRequest::FocusTab)
    }

    pub fn file_open_request(&self, position: ClientMousePosition) -> Option<ClientRequest> {
        let region = self.pane_regions.pane_at(position)?;
        let tab = self
            .layout
            .tabs()
            .iter()
            .find(|tab| *tab.id() == *self.layout.active_tab())?;
        let pane = tab.panes().iter().find(|pane| pane.id == *region.id())?;
        let link = self.frame_buffer.file_link_at_resolved(region, position, &pane.cwd)?;
        Some(ClientRequest::OpenFile {
            pane_id: *region.id(),
            path: link.path.to_str()?.to_owned(),
            line: link.line,
            column: link.column,
        })
    }

    pub const fn begin_file_open_capture(&mut self) {
        self.file_open_capture = Some(FileOpenCapture::Armed);
    }

    pub const fn finish_file_open_capture(&mut self) -> FileOpenRelease {
        match self.file_open_capture.take() {
            Some(FileOpenCapture::Armed) => FileOpenRelease::Consumed,
            None => FileOpenRelease::NotCaptured,
        }
    }

    pub fn apply_pane_regions_logical(&mut self, pane_regions: PaneRegionsSnapshot) {
        self.pane_regions = pane_regions;
        self.clicks.retain_for_regions(&self.pane_regions);
        self.mouse_capture = self
            .mouse_capture
            .take()
            .and_then(|capture| capture.retain_for_regions(&self.pane_regions));
        self.selection_edge_scroll.retain_for_regions(&self.pane_regions);
        self.selection.clear_if_regions_changed(&self.pane_regions);
        self.sync_mouse_capture_logical();
    }

    pub fn sync_mouse_capture_logical(&mut self) {
        self.any_motion_capture = self.next_mouse_capture();
    }

    fn next_mouse_capture(&self) -> MouseAnyMotionCapture {
        if self
            .pane_regions
            .regions()
            .iter()
            .any(|region| region.mouse_mode() == PaneMouseMode::AnyMotion)
        {
            MouseAnyMotionCapture::Enabled
        } else {
            MouseAnyMotionCapture::Disabled
        }
    }

    pub fn apply_render_logical(&mut self, update: RenderUpdate) -> rootcause::Result<ClientRenderOutcome> {
        match self.frame_buffer.apply(update)? {
            ApplyOutcome::Applied(_) => {
                self.render_generation = self
                    .render_generation
                    .checked_add(1)
                    .ok_or_else(|| rootcause::report!("muxr client render generation overflowed"))?;
                if self.selection_edge_scroll.waits_for_render() {
                    self.edge_scroll_render_generation = Some(self.render_generation);
                }
                self.selection.refresh_visible_rows(&self.frame_buffer)?;
                self.refresh_edge_drag_selection_logical()?;
                self.tab_bar_dmg = TabBarDmg::Clean;
                Ok(ClientRenderOutcome::Drawn)
            }
            ApplyOutcome::NeedsResync => Ok(ClientRenderOutcome::NeedsResync),
        }
    }

    pub fn presentation_snapshot(&self) -> ClientPresentationSnapshot {
        ClientPresentationSnapshot {
            any_motion_capture: self.any_motion_capture,
            edge_scroll_pending: self.selection_edge_scroll.render_pending().cloned(),
            frame_buffer: self.frame_buffer.clone(),
            layout: self.layout.clone(),
            edge_scroll_render_generation: self.edge_scroll_render_generation,
            selection: self.selection.range().cloned(),
        }
    }

    pub fn presentation_transaction(
        &mut self,
        committed: Option<&ClientPresentationSnapshot>,
    ) -> rootcause::Result<Option<Vec<u8>>> {
        if self.tab_bar_dmg == TabBarDmg::Dirty {
            return Ok(None);
        }
        let logical = self.presentation_snapshot();
        let mouse_changed = committed.is_none_or(|previous| previous.any_motion_capture != logical.any_motion_capture);
        let Some(changes) = self.presentation_changes(committed, &logical)? else {
            if !mouse_changed {
                return Ok(None);
            }
            let mut transaction = Vec::new();
            crate::terminal::queue_mouse_any_motion_capture(&mut transaction, logical.any_motion_capture)?;
            return Ok(Some(transaction));
        };
        let render_tab_bar = committed.is_none_or(|previous| previous.layout != logical.layout)
            || changes.scope() == RenderFrameScope::Full;
        self.render_transaction.clear();
        let result = (|| {
            if mouse_changed {
                crate::terminal::queue_mouse_any_motion_capture(
                    &mut self.render_transaction,
                    logical.any_motion_capture,
                )?;
            }
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
            self.terminal_encoder.encode(
                &mut self.render_transaction,
                TerminalRender {
                    changes: &changes,
                    frame_buffer: &self.frame_buffer,
                    origin: TerminalOrigin {
                        col: self.tab_bar_config.width,
                        row: 0,
                    },
                    selection,
                },
            )?;
            crate::terminal::queue_synchronized_update_end(&mut self.render_transaction, self.synchronized_output)
        })();
        let transaction = result.map(|()| self.render_transaction.clone());
        self.reset_render_transaction();
        transaction.map(Some)
    }

    pub fn acknowledge_presentation(&mut self, snapshot: &ClientPresentationSnapshot) {
        if snapshot.edge_scroll_render_generation == self.edge_scroll_render_generation
            && snapshot.edge_scroll_render_generation.is_some()
            && let Some(pending) = snapshot.edge_scroll_pending.as_ref()
        {
            self.selection_edge_scroll.clear_render_acknowledged_pending(pending);
            self.edge_scroll_render_generation = None;
        }
    }

    fn reset_render_transaction(&mut self) {
        self.render_transaction.clear();
        if self.render_transaction.capacity() > MAX_RETAINED_RENDER_TRANSACTION_BYTES {
            self.render_transaction = Vec::new();
        }
    }

    pub fn apply_selection_input_logical(&mut self, input: SelectionInput) -> rootcause::Result<()> {
        self.apply_selection_input_at_logical(input, Instant::now())
    }

    pub(crate) fn clear_selection(&mut self) {
        self.clicks.reset();
        self.selection.clear();
        self.selection_edge_scroll.clear();
        self.edge_scroll_render_generation = None;
    }

    pub(crate) fn apply_selection_input_at_logical(
        &mut self,
        input: SelectionInput,
        now: Instant,
    ) -> rootcause::Result<()> {
        if matches!(input, SelectionInput::Start(_) | SelectionInput::End(_)) {
            self.selection_edge_scroll.clear();
            self.edge_scroll_render_generation = None;
        }
        match input {
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

    fn refresh_edge_drag_selection_logical(&mut self) -> rootcause::Result<()> {
        let Some(position) = self.selection_edge_scroll.drag_position(&self.pane_regions) else {
            self.selection_edge_scroll.clear();
            self.edge_scroll_render_generation = None;
            return Ok(());
        };
        self.apply_selection_input_logical(SelectionInput::Update(position))
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

    fn presentation_changes(
        &self,
        committed: Option<&ClientPresentationSnapshot>,
        logical: &ClientPresentationSnapshot,
    ) -> rootcause::Result<Option<RenderFrameChanges>> {
        let Some(committed) = committed else {
            return Ok(self.frame_buffer.full_redraw_changes());
        };
        let Some(mut rows) = self.frame_buffer.changed_rows_since(&committed.frame_buffer) else {
            return Ok(self.frame_buffer.full_redraw_changes());
        };
        if committed.selection != logical.selection {
            rows.extend(crate::copy_selection::changed_selection_rows(
                committed.selection.as_ref(),
                logical.selection.as_ref(),
            ));
        }
        let rows = rows
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return if committed.layout == logical.layout {
                if self.frame_buffer.cursor_matches(&committed.frame_buffer) {
                    Ok(None)
                } else {
                    self.frame_buffer.row_redraw_changes(&[])
                }
            } else {
                self.frame_buffer.row_redraw_changes(&[])
            };
        }
        self.frame_buffer.row_redraw_changes(&rows)
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
    use muxr_core::RenderBaseline;
    use muxr_core::TabId;
    use muxr_core::TabSnapshot;
    use muxr_core::TerminalSize;
    use rootcause::report;
    use test_that::prelude::*;

    use super::*;
    use crate::renderer::test_helpers;

    #[test]
    fn test_file_open_request_when_bare_file_is_clicked_resolves_against_pane_cwd() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("README");
        std::fs::write(&path, b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| report!("temporary test directory is not UTF-8"))?;

        let request = self::file_open_request_for_text(cwd, "README")?;

        assert_that!(
            request,
            some(eq(ClientRequest::OpenFile {
                pane_id: PaneId::new(1)?,
                path: path
                    .to_str()
                    .ok_or_else(|| report!("temporary test file path is not UTF-8"))?
                    .to_owned(),
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_file_open_request_when_bare_directory_is_clicked_resolves_against_pane_cwd() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("src");
        std::fs::create_dir(&path)?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| report!("temporary test directory is not UTF-8"))?;

        let request = self::file_open_request_for_text(cwd, "src")?;

        assert_that!(
            request,
            some(eq(ClientRequest::OpenFile {
                pane_id: PaneId::new(1)?,
                path: path
                    .to_str()
                    .ok_or_else(|| report!("temporary directory path is not UTF-8"))?
                    .to_owned(),
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    fn file_open_request_for_text(cwd: &str, text: &str) -> rootcause::Result<Option<ClientRequest>> {
        let width = u16::try_from(text.chars().count())?;
        let layout = LayoutSnapshot::new(
            TabId::new(1)?,
            vec![TabSnapshot::new(
                TabId::new(1)?,
                "default",
                PaneId::new(1)?,
                vec![PaneSnapshot {
                    tracked_process_state: muxr_core::TrackedProcessState::None,
                    cwd: cwd.to_owned(),
                    cmd_label: None,
                    focus_seq: 1,
                    id: PaneId::new(1)?,
                    title: "shell".to_owned(),
                }],
            )?],
        )?;
        let pane_regions = PaneRegionsSnapshot::new(vec![PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            width,
            1,
            PaneMouseMode::None,
            0,
        )?])?;
        let mut renderer = ClientRenderer::with_synchronized_output(layout, pane_regions, SynchronizedOutput::Csi);
        let baseline = RenderBaseline::new(
            1,
            muxr_core::TerminalSize::new(width, 1)?,
            muxr_core::RenderCursor {
                row: 0,
                col: 0,
                shape: muxr_core::RenderCursorShape::Default,
                visibility: muxr_core::RenderCursorVisibility::Visible,
            },
            vec![muxr_core::RenderRowSpan::new(
                0,
                0,
                text.chars()
                    .map(|ch| muxr_core::RenderCell::narrow(ch.to_string(), muxr_core::RenderStyle::default()))
                    .collect(),
            )?],
        )?;
        renderer.apply_render_logical(RenderUpdate::Baseline(baseline))?;
        Ok(renderer.file_open_request(ClientMousePosition { row: 0, col: 1 }))
    }

    #[test]
    fn test_presentation_transaction_when_layout_is_dirty_encodes_one_complete_frame() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_layout(two_tab_layout()?);
        let outcome = renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        let transaction = renderer
            .presentation_transaction(None)?
            .ok_or_else(|| report!("expected presentation transaction"))?;

        assert_that!(outcome, eq(ClientRenderOutcome::Drawn));
        let terminal_output = String::from_utf8(transaction)?;
        assert_that!(terminal_output, contains_substring("\x1b[?2026h"));
        assert_that!(terminal_output, ends_with("\x1b[?2026l"));
        let tab_bar_index = terminal_output.find("tab-1").unwrap_or(usize::MAX);
        let pane_index = terminal_output.find("ab").unwrap_or(usize::MAX);
        assert_that!(terminal_output, not(contains_substring("\x1b[2J")));
        assert_that!(tab_bar_index, lt(pane_index));
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
    fn test_client_renderer_apply_render_logical_when_resync_is_needed_returns_needs_resync() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let outcome = renderer.apply_render_logical(muxr_core::RenderUpdate::Diff(render_diff()?))?;

        assert_that!(outcome, eq(ClientRenderOutcome::NeedsResync));
        Ok(())
    }

    #[test]
    fn test_presentation_transaction_when_pane_regions_need_mouse_capture_defers_until_first_render()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_pane_regions_logical(any_motion_pane_regions_snapshot()?);
        assert_that!(renderer.presentation_transaction(None)?, eq(None));
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_pane_regions_when_any_motion_is_no_longer_needed_reasserts_outer_capture()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            any_motion_pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer.sync_mouse_capture_logical();
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        let committed = renderer.presentation_snapshot();
        renderer.apply_pane_regions_logical(pane_regions_snapshot()?);
        let transaction = renderer
            .presentation_transaction(Some(&committed))?
            .ok_or_else(|| report!("expected mouse capture transaction"))?;

        assert_that!(
            String::from_utf8(transaction)?,
            eq("\x1b[?1003l\x1b[?1000h\x1b[?1002h\x1b[?1006h")
        );
        Ok(())
    }

    #[test]
    fn test_presentation_transaction_when_initial_frame_reasserts_current_mouse_capture_before_frame()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        renderer.apply_pane_regions_logical(any_motion_pane_regions_snapshot()?);
        let transaction = renderer
            .presentation_transaction(None)?
            .ok_or_else(|| report!("expected a full redraw transaction for the current frame"))?;
        let terminal_output = String::from_utf8(transaction)?;

        assert_that!(terminal_output, starts_with("\x1b[?1003h\x1b[?2026h"));
        assert_that!(terminal_output, not(contains_substring("\x1b[2J")));
        Ok(())
    }

    #[test]
    fn test_presentation_transaction_when_initial_frame_arrives_encodes_full_repaint_without_clear()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );

        assert_that!(
            renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(render_baseline()?))?,
            eq(ClientRenderOutcome::Drawn)
        );
        let transaction = renderer
            .presentation_transaction(None)?
            .ok_or_else(|| report!("expected initial presentation transaction"))?;
        let terminal_output = String::from_utf8(transaction)?;

        assert_that!(terminal_output, contains_substring("\x1b[?2026h"));
        assert_that!(terminal_output, not(contains_substring("\x1b[2J")));
        Ok(())
    }

    #[test]
    fn test_presentation_transaction_when_selection_changes_encodes_delta_from_committed_snapshot()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        let committed = renderer.presentation_snapshot();
        renderer.apply_selection_input_logical(SelectionInput::Start(ClientMousePosition { row: 0, col: 0 }))?;
        renderer.apply_selection_input_logical(SelectionInput::Update(ClientMousePosition { row: 0, col: 1 }))?;

        let transaction = renderer
            .presentation_transaction(Some(&committed))?
            .ok_or_else(|| report!("expected selection presentation transaction"))?;
        let terminal_output = String::from_utf8(transaction)?;

        assert_that!(terminal_output, starts_with("\x1b[?2026h"));
        assert_that!(terminal_output, not(contains_substring("tab-0")));
        assert_that!(terminal_output, not(contains_substring("\x1b[2J")));
        Ok(())
    }

    #[test]
    fn test_client_renderer_clear_selection_when_selection_exists_removes_selection() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        renderer.apply_selection_input_logical(SelectionInput::Start(ClientMousePosition { row: 0, col: 0 }))?;
        renderer.apply_selection_input_logical(SelectionInput::Update(ClientMousePosition { row: 0, col: 1 }))?;

        renderer.clear_selection();

        assert_that!(test_helpers::selection_contains(&renderer, 0, 0), eq(false));
        assert_that!(test_helpers::selected_text(&renderer), eq(None));
        Ok(())
    }

    #[test]
    fn test_client_renderer_when_region_diff_is_pixel_identical_skips_terminal_transaction() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        let committed = renderer.presentation_snapshot();

        renderer.apply_render_logical(muxr_core::RenderUpdate::Diff(muxr_core::RenderDiff::new(
            1,
            2,
            TerminalSize::new(2, 1)?,
            muxr_core::RenderCursor {
                row: 0,
                col: 1,
                shape: muxr_core::RenderCursorShape::Default,
                visibility: muxr_core::RenderCursorVisibility::Visible,
            },
            Vec::new(),
        )?))?;

        assert_that!(renderer.presentation_transaction(Some(&committed))?, eq(None));
        Ok(())
    }

    #[test]
    fn test_client_renderer_when_sidebar_layout_changes_encodes_sidebar_only_transaction() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        let committed = renderer.presentation_snapshot();

        renderer.apply_sidebar_layout_logical(two_tab_layout()?);

        let transaction = renderer
            .presentation_transaction(Some(&committed))?
            .ok_or_else(|| report!("expected sidebar-only presentation transaction"))?;
        let terminal_output = String::from_utf8(transaction)?;

        assert_that!(terminal_output, contains_substring("tab-1"));
        assert_that!(terminal_output, not(contains_substring("\x1b[2J")));
        Ok(())
    }

    #[test]
    fn test_client_renderer_when_sidebar_follows_layout_defers_presentation_until_render() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        let committed = renderer.presentation_snapshot();

        let layout = two_tab_layout()?;
        renderer.apply_layout(layout.clone());
        renderer.apply_sidebar_layout_logical(layout);

        assert_that!(renderer.presentation_transaction(Some(&committed))?, eq(None));
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
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        let committed = renderer.presentation_snapshot();
        renderer
            .apply_selection_input_logical(SelectionInput::Start(muxr_core::ClientMousePosition { row: 0, col: 0 }))?;
        renderer.apply_selection_input_logical(SelectionInput::Update(muxr_core::ClientMousePosition {
            row: 0,
            col: 1,
        }))?;

        assert_that!(test_helpers::selection_contains(&renderer, 0, 0), eq(true));
        assert_that!(test_helpers::selection_contains(&renderer, 0, 1), eq(true));
        let selection_output = String::from_utf8(
            renderer
                .presentation_transaction(Some(&committed))?
                .ok_or_else(|| report!("expected selection transaction"))?,
        )?;
        assert_that!(selection_output, not(contains_substring("\x1b[7m")));
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
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        renderer
            .apply_selection_input_logical(SelectionInput::Start(muxr_core::ClientMousePosition { row: 0, col: 0 }))?;
        renderer
            .apply_selection_input_logical(SelectionInput::End(muxr_core::ClientMousePosition { row: 0, col: 1 }))?;
        let committed = renderer.presentation_snapshot();
        renderer.apply_pane_regions_logical(pane_regions_snapshot_with_visible_top_row(1)?);

        let redrawn = String::from_utf8(
            renderer
                .presentation_transaction(Some(&committed))?
                .ok_or_else(|| report!("expected selection viewport transaction"))?,
        )?;
        assert_that!(redrawn, contains_substring("ab"));
        assert_that!(redrawn, not(contains_substring("\x1b[7m")));
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
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(three_row_render_baseline(
            "aa", "bb", "cc",
        )?))?;
        renderer.apply_selection_input_logical(SelectionInput::Start(ClientMousePosition { row: 0, col: 0 }))?;
        let scroll_request = renderer
            .set_selection_edge_drag(ClientMousePosition { row: 3, col: 1 }, None)
            .map(|request| request.into_parts().1);
        renderer.apply_selection_input_logical(SelectionInput::Update(ClientMousePosition { row: 3, col: 1 }))?;

        assert_that!(
            scroll_request,
            eq(Some(ClientRequest::ScrollPaneLineAt {
                direction: PaneScrollDirection::Down,
                position: ClientMousePosition { row: 2, col: 1 },
            }))
        );

        renderer.apply_pane_regions_logical(three_row_pane_regions_snapshot(10)?);
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(three_row_render_baseline(
            "bb", "cc", "dd",
        )?))?;

        assert_that!(
            test_helpers::selected_text(&renderer),
            eq(Some("aa\nbb\ncc\ndd".to_owned()))
        );
        assert_that!(test_helpers::selection_contains(&renderer, 2, 0), eq(true));
        Ok(())
    }

    #[test]
    fn test_client_renderer_acknowledge_presentation_when_edge_scroll_render_flushes_releases_next_request()
    -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            three_row_pane_regions_snapshot(9)?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(three_row_render_baseline(
            "aa", "bb", "cc",
        )?))?;
        renderer.apply_selection_input_logical(SelectionInput::Start(ClientMousePosition { row: 0, col: 0 }))?;
        let request = renderer
            .set_selection_edge_drag(ClientMousePosition { row: 3, col: 1 }, None)
            .ok_or_else(|| report!("expected edge-scroll request"))?;
        let (pending, _) = request.into_parts();
        renderer.mark_selection_edge_scroll_sent(pending);
        renderer.apply_pane_regions_logical(three_row_pane_regions_snapshot(10)?);

        let before_render = renderer.presentation_snapshot();
        renderer.acknowledge_presentation(&before_render);
        assert_that!(renderer.selection_edge_scroll_request(), eq(None));

        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(three_row_render_baseline(
            "bb", "cc", "dd",
        )?))?;
        let flushed_snapshot = renderer.presentation_snapshot();
        renderer.acknowledge_presentation(&flushed_snapshot);

        assert_that!(renderer.selection_edge_scroll_request().is_some(), eq(true));
        Ok(())
    }

    #[test]
    fn test_client_renderer_when_old_edge_scroll_flushes_keeps_new_pending_request() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            three_row_pane_regions_snapshot(9)?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(three_row_render_baseline(
            "aa", "bb", "cc",
        )?))?;
        renderer.apply_selection_input_logical(SelectionInput::Start(ClientMousePosition { row: 0, col: 0 }))?;

        let first_request = renderer
            .set_selection_edge_drag(ClientMousePosition { row: 2, col: 1 }, None)
            .ok_or_else(|| report!("expected first edge-scroll request"))?;
        let (first_pending, _) = first_request.into_parts();
        renderer.mark_selection_edge_scroll_sent(first_pending);
        renderer.apply_pane_regions_logical(three_row_pane_regions_snapshot(10)?);
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(three_row_render_baseline(
            "bb", "cc", "dd",
        )?))?;
        let first_snapshot = renderer.presentation_snapshot();

        let second_request = renderer
            .set_selection_edge_drag(ClientMousePosition { row: 0, col: 1 }, None)
            .ok_or_else(|| report!("expected second edge-scroll request"))?;
        let (second_pending, _) = second_request.into_parts();
        renderer.mark_selection_edge_scroll_sent(second_pending);
        renderer.apply_pane_regions_logical(three_row_pane_regions_snapshot(9)?);

        renderer.acknowledge_presentation(&first_snapshot);
        assert_that!(renderer.selection_edge_scroll_request(), eq(None));

        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(three_row_render_baseline(
            "aa", "bb", "cc",
        )?))?;
        let second_snapshot = renderer.presentation_snapshot();
        renderer.acknowledge_presentation(&second_snapshot);

        assert_that!(renderer.selection_edge_scroll_request().is_some(), eq(true));
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
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(three_row_render_baseline(
            "aa", "bb", "cc",
        )?))?;
        renderer.apply_selection_input_logical(SelectionInput::Start(ClientMousePosition { row: 1, col: 0 }))?;

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
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(three_row_render_baseline(
            "aa", "bb", "cc",
        )?))?;
        renderer.apply_selection_input_logical(SelectionInput::Start(ClientMousePosition { row: 1, col: 0 }))?;
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
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(three_row_render_baseline(
            "aa", "bb", "cc",
        )?))?;
        renderer.apply_selection_input_logical(SelectionInput::Start(ClientMousePosition { row: 1, col: 0 }))?;
        let position = ClientMousePosition { row: 2, col: 1 };
        let direction = PaneScrollDirection::Down;
        let request = renderer
            .set_selection_edge_drag(position, None)
            .ok_or_else(|| report!("expected muxr edge scroll request"))?;
        let (pending, _) = request.into_parts();

        renderer.mark_selection_edge_scroll_sent(pending);
        renderer.apply_scroll_pane_line_result(position, direction, PaneScrollLineMove::Moved);

        assert_that!(renderer.selection_edge_scroll_request(), eq(None));
        renderer.apply_pane_regions_logical(three_row_pane_regions_snapshot(10)?);
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(three_row_render_baseline(
            "bb", "cc", "dd",
        )?))?;
        let rendered_snapshot = renderer.presentation_snapshot();
        renderer.acknowledge_presentation(&rendered_snapshot);
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
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(word_render_baseline()?))?;
        let now = Instant::now();
        let first_position = ClientMousePosition { row: 0, col: first_col };
        let second_position = ClientMousePosition {
            row: 0,
            col: second_col,
        };
        let second_click_at = now
            .checked_add(Duration::from_millis(100))
            .ok_or_else(|| report!("muxr double-click selection test instant overflowed"))?;
        renderer.apply_selection_input_at_logical(SelectionInput::Start(first_position), now)?;
        renderer.apply_selection_input_at_logical(SelectionInput::End(first_position), now)?;
        renderer.apply_selection_input_at_logical(SelectionInput::Start(second_position), second_click_at)?;

        assert_that!(test_helpers::selected_text(&renderer), eq(Some("two".to_owned())));
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_pane_regions_when_same_pane_remains_keeps_double_click() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            word_pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        renderer.apply_render_logical(muxr_core::RenderUpdate::Baseline(word_render_baseline()?))?;
        let now = Instant::now();
        let position = ClientMousePosition { row: 0, col: 4 };
        let second_click_at = now
            .checked_add(Duration::from_millis(100))
            .ok_or_else(|| report!("muxr retained double-click selection test instant overflowed"))?;
        renderer.apply_selection_input_at_logical(SelectionInput::Start(position), now)?;
        renderer.apply_selection_input_at_logical(SelectionInput::End(position), now)?;
        renderer.apply_pane_regions_logical(word_pane_regions_snapshot()?);
        renderer.apply_selection_input_at_logical(SelectionInput::Start(position), second_click_at)?;

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
}
