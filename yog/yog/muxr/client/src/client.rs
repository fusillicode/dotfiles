use std::collections::BTreeSet;
use std::fs;
use std::future::Future;
use std::io::IsTerminal;
use std::io::Read;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use muxr_core::AttachRequest;
use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::ClientMousePosition;
use muxr_core::ClientRequest;
use muxr_core::INTERNAL_SERVER_ARG;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::PaneScrollDirection;
use muxr_core::RenderUpdate;
use muxr_core::ServerEvent;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use muxr_core::TerminalSize;
use muxr_transport::ClientConnection;
use muxr_transport::ClientEventReader;
use muxr_transport::ClientRequestWriter;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::copy::SelectionInput;
use crate::copy::SelectionRange;
use crate::copy::SelectionState;
use crate::input::DecodedInput;
use crate::input::InputDecoder;
use crate::render::ApplyOutcome;
use crate::render::FrameBuffer;
use crate::render::SynchronizedOutput;

const RESIZE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const ATTACH_TIMEOUT: Duration = Duration::from_secs(2);
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(2);
const AMBIGUOUS_INPUT_TIMEOUT: Duration = Duration::from_millis(50);
const DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(400);
const SELECTION_EDGE_SCROLL_INTERVAL: Duration = Duration::from_millis(50);
const STDIN_BUFFER_SIZE: usize = 8192;
const TAB_BAR_ROWS: u16 = 1;
const CONTROL_REQUEST_CHANNEL_LIMIT: usize = 128;
const INPUT_REQUEST_CHANNEL_LIMIT: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
enum StdinRead {
    Bytes(Vec<u8>),
    Eof,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ClientInputAction {
    CopySelection,
    Mouse(ClientMouseEvent),
    ServerRequest(ClientRequest),
}

/// Start or attach to a muxr session and run an interactive client.
///
/// # Errors
/// - The session paths cannot be resolved.
/// - The server cannot be started or attached.
/// - The current terminal size cannot be read.
/// - Terminal input/output or protocol IO fails.
pub fn start(session: &SessionName, server_executable: &Path) -> rootcause::Result<()> {
    self::run_async(async {
        let terminal_size = self::current_terminal_size()?;
        let pane_size = self::pane_size_for_terminal(&terminal_size)?;
        let attached_session = self::open_session(session, pane_size.clone(), server_executable).await?;
        self::run_interactive(attached_session, pane_size).await
    })
}

struct AttachedSession {
    layout: LayoutSnapshot,
    pane_regions: PaneRegionsSnapshot,
    reader: ClientEventReader,
    writer: ClientRequestWriter,
}

enum AttachFailure {
    Rejected(rootcause::Report),
    Unusable(rootcause::Report),
}

struct TerminalGuard {
    entered_render_screen: bool,
    raw_mode_enabled: bool,
}

impl TerminalGuard {
    fn enable_if_terminal() -> rootcause::Result<Self> {
        let raw_mode_enabled = std::io::stdin().is_terminal();
        if raw_mode_enabled {
            crossterm::terminal::enable_raw_mode().context("failed to enable muxr client raw mode")?;
        }
        let entered_render_screen = std::io::stdout().is_terminal();
        if entered_render_screen {
            let mut stdout = std::io::stdout();
            if let Err(error) = crate::render::enter_terminal(&mut stdout) {
                if raw_mode_enabled {
                    drop(crossterm::terminal::disable_raw_mode());
                }
                return Err(error).context("failed to enter muxr client terminal screen")?;
            }
        }

        Ok(Self {
            entered_render_screen,
            raw_mode_enabled,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.entered_render_screen {
            let mut stdout = std::io::stdout();
            drop(crate::render::restore_terminal(&mut stdout));
        }
        if self.raw_mode_enabled {
            drop(crossterm::terminal::disable_raw_mode());
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ClientRenderOutcome {
    Drawn,
    NeedsResync,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClickKind {
    Double,
    Other,
}

#[derive(Clone, Debug, Default)]
struct ClickTracker {
    count: u8,
    previous: Option<TrackedClick>,
}

impl ClickTracker {
    fn record(&mut self, target: ClickTarget, now: Instant) -> ClickKind {
        let continues_previous = self.previous.as_ref().is_some_and(|previous| {
            previous.target == target
                && now
                    .checked_duration_since(previous.at)
                    .is_some_and(|elapsed| elapsed <= DOUBLE_CLICK_THRESHOLD)
        });
        self.count = if continues_previous {
            self.count.saturating_add(1)
        } else {
            1
        };
        self.previous = Some(TrackedClick { at: now, target });
        if self.count == 2 {
            ClickKind::Double
        } else {
            ClickKind::Other
        }
    }

    fn retain_for_regions(&mut self, regions: &PaneRegionsSnapshot) {
        let keep_previous = self
            .previous
            .as_ref()
            .is_some_and(|previous| previous.target.remains_in_regions(regions));
        if !keep_previous {
            self.reset();
        }
    }

    fn reset(&mut self) {
        self.count = 0;
        self.previous = None;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ClickTarget {
    Cell {
        pane_id: PaneId,
        position: ClientMousePosition,
    },
    Word {
        end: ClientMousePosition,
        pane_id: PaneId,
        start: ClientMousePosition,
    },
}

impl ClickTarget {
    fn remains_in_regions(&self, regions: &PaneRegionsSnapshot) -> bool {
        match self {
            Self::Cell { pane_id, position } => regions.pane_at(*position).is_some_and(|region| region.id() == pane_id),
            Self::Word { end, pane_id, start } => self::region_for_pane_id(regions, pane_id)
                .is_some_and(|region| region.contains(start.row, start.col) && region.contains(end.row, end.col)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MouseCapture {
    region: PaneRegionSnapshot,
}

impl MouseCapture {
    fn retain_for_regions(self, regions: &PaneRegionsSnapshot) -> Option<Self> {
        self::region_for_pane_id(regions, self.region.id())
            .cloned()
            .map(|region| Self { region })
    }
}

#[derive(Clone, Debug)]
struct TrackedClick {
    at: Instant,
    target: ClickTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SelectionEdgeDrag {
    col: u16,
    direction: PaneScrollDirection,
    pane_id: PaneId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SelectionEdgeScrollPending {
    direction: PaneScrollDirection,
    pane_id: PaneId,
    previous_visible_top_row: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SelectionEdgeScrollRequest {
    pending: SelectionEdgeScrollPending,
    request: ClientRequest,
}

struct ClientRenderer {
    any_motion_capture_enabled: bool,
    chrome_dirty: bool,
    clicks: ClickTracker,
    frame_buffer: FrameBuffer,
    layout: LayoutSnapshot,
    mouse_capture: Option<MouseCapture>,
    pane_regions: PaneRegionsSnapshot,
    selection_edge_drag: Option<SelectionEdgeDrag>,
    selection_edge_scroll_acknowledged: bool,
    selection_edge_scroll_pending: Option<SelectionEdgeScrollPending>,
    selection: SelectionState,
    synchronized_output: SynchronizedOutput,
}

impl ClientRenderer {
    fn new(layout: LayoutSnapshot, pane_regions: PaneRegionsSnapshot) -> Self {
        Self::with_synchronized_output(
            layout,
            pane_regions,
            SynchronizedOutput::for_term(std::env::var("TERM").ok().as_deref()),
        )
    }

    fn with_synchronized_output(
        layout: LayoutSnapshot,
        pane_regions: PaneRegionsSnapshot,
        synchronized_output: SynchronizedOutput,
    ) -> Self {
        Self {
            any_motion_capture_enabled: false,
            chrome_dirty: true,
            clicks: ClickTracker::default(),
            frame_buffer: FrameBuffer::default(),
            layout,
            mouse_capture: None,
            pane_regions,
            selection_edge_drag: None,
            selection_edge_scroll_acknowledged: false,
            selection_edge_scroll_pending: None,
            selection: SelectionState::default(),
            synchronized_output,
        }
    }

    fn apply_layout(&mut self, layout: LayoutSnapshot) {
        // Layout events precede their matching render baseline; defer chrome writes so the user never sees new tab
        // state over an old pane frame.
        self.layout = layout;
        self.chrome_dirty = true;
    }

    fn apply_pane_regions(
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
        self.selection_edge_drag = self
            .selection_edge_drag
            .take()
            .and_then(|drag| self::region_for_pane_id(&self.pane_regions, &drag.pane_id).map(|_| drag));
        self.update_selection_edge_scroll_pending();
        let selection_changed = self.selection.clear_if_regions_changed(&self.pane_regions);
        self.sync_mouse_capture(stdout)?;
        if selection_changed {
            let next_selection = self.selection.range().cloned();
            self.redraw_selection(stdout, previous_selection.as_ref(), next_selection.as_ref())?;
        }
        Ok(())
    }

    fn sync_mouse_capture(&mut self, stdout: &mut impl Write) -> rootcause::Result<()> {
        let next = self
            .pane_regions
            .regions()
            .iter()
            .any(|region| region.mouse_mode().needs_any_motion_capture());
        if self.any_motion_capture_enabled == next {
            return Ok(());
        }

        crate::render::set_mouse_any_motion_capture(stdout, next)?;
        self.any_motion_capture_enabled = next;
        Ok(())
    }

    fn apply_render(
        &mut self,
        stdout: &mut impl Write,
        update: RenderUpdate,
    ) -> rootcause::Result<ClientRenderOutcome> {
        match self.frame_buffer.apply(update)? {
            ApplyOutcome::Applied(changes) => {
                self.selection.refresh_visible_rows(&self.frame_buffer)?;
                self.draw(stdout, &changes)?;
                self.refresh_edge_drag_selection(stdout)?;
                if self.selection_edge_scroll_acknowledged {
                    self.selection_edge_scroll_acknowledged = false;
                    self.selection_edge_scroll_pending = None;
                }
                Ok(ClientRenderOutcome::Drawn)
            }
            ApplyOutcome::NeedsResync => Ok(ClientRenderOutcome::NeedsResync),
        }
    }

    fn draw(&mut self, stdout: &mut impl Write, changes: &crate::render::RenderFrameChanges) -> rootcause::Result<()> {
        let render_chrome = self.chrome_dirty || changes.is_full_redraw();
        let mut frame = Vec::new();
        crate::render::queue_synchronized_update_start(&mut frame, self.synchronized_output)?;
        if changes.is_full_redraw() {
            crate::render::queue_full_redraw_start(&mut frame)?;
        }
        if render_chrome {
            crate::tab_bar::queue(&mut frame, &self.layout)?;
        }
        self.frame_buffer
            .queue_at_with_selection(&mut frame, changes, TAB_BAR_ROWS, self.selection.range())?;
        crate::render::queue_synchronized_update_end(&mut frame, self.synchronized_output)?;
        stdout
            .write_all(&frame)
            .context("failed to write muxr client render transaction")?;
        stdout
            .flush()
            .context("failed to flush muxr client render transaction")?;
        self.chrome_dirty = false;
        Ok(())
    }

    fn apply_selection_input(&mut self, stdout: &mut impl Write, input: SelectionInput) -> rootcause::Result<()> {
        self.apply_selection_input_at(stdout, input, Instant::now())
    }

    fn apply_selection_input_at(
        &mut self,
        stdout: &mut impl Write,
        input: SelectionInput,
        now: Instant,
    ) -> rootcause::Result<()> {
        let previous_selection = self.selection.range().cloned();
        if matches!(input, SelectionInput::Start(_) | SelectionInput::End(_)) {
            self.selection_edge_drag = None;
            self.selection_edge_scroll_acknowledged = false;
            self.selection_edge_scroll_pending = None;
        }
        let changed = match input {
            SelectionInput::Start(position) if self.record_click(position, now) == ClickKind::Double => self
                .selection
                .select_word(position, &self.pane_regions, &self.frame_buffer)?,
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
        if changed {
            let next_selection = self.selection.range().cloned();
            self.redraw_selection(stdout, previous_selection.as_ref(), next_selection.as_ref())?;
        }
        Ok(())
    }

    fn record_click(&mut self, position: ClientMousePosition, now: Instant) -> ClickKind {
        let Some(region) = self.pane_regions.pane_at(position) else {
            self.clicks.reset();
            return ClickKind::Other;
        };
        self.clicks.record(self.click_target(position, region), now)
    }

    fn click_target(&self, position: ClientMousePosition, region: &PaneRegionSnapshot) -> ClickTarget {
        if let Some(selection) = crate::copy::word_selection_at(position, &self.pane_regions, &self.frame_buffer)
            && let Some((start, end)) = selection.bounds_positions()
        {
            return ClickTarget::Word {
                end,
                pane_id: selection.pane_id().clone(),
                start,
            };
        }

        ClickTarget::Cell {
            pane_id: region.id().clone(),
            position,
        }
    }

    fn mouse_request_for_event(&mut self, event: ClientMouseEvent) -> Option<ClientMouseEvent> {
        if let Some(capture) = self.mouse_capture.as_ref() {
            let event = event.with_position(self::clamp_mouse_position_to_region(event.position(), &capture.region));
            if event.phase() == ClientMouseEventPhase::Release {
                self.mouse_capture = None;
            }
            return Some(event);
        }

        let region = self.pane_regions.pane_at(event.position())?;
        if !region.mouse_tracking_enabled() {
            return None;
        }
        if self::mouse_event_starts_capture(event) {
            self.mouse_capture = Some(MouseCapture { region: region.clone() });
        }
        Some(event)
    }

    fn copy_selection(&self) -> rootcause::Result<()> {
        let Some(text) = self.selection.selected_text() else {
            return Ok(());
        };
        crate::copy::copy_to_clipboard(&text)
    }

    fn set_selection_edge_drag(
        &mut self,
        position: ClientMousePosition,
        forced_direction: Option<PaneScrollDirection>,
    ) -> Option<SelectionEdgeScrollRequest> {
        let Some(region) = self.selection.drag_region() else {
            self.selection_edge_scroll_acknowledged = false;
            self.selection_edge_scroll_pending = None;
            return None;
        };
        let (direction, row) = if let Some(direction) = forced_direction {
            (direction, self::selection_edge_row(region, direction))
        } else if position.row < region.row() {
            (PaneScrollDirection::Up, region.row())
        } else if position.row > self::last_region_row_saturating(region) {
            (PaneScrollDirection::Down, self::last_region_row_saturating(region))
        } else {
            self.selection_edge_drag = None;
            self.selection_edge_scroll_acknowledged = false;
            self.selection_edge_scroll_pending = None;
            return None;
        };
        if self
            .selection_edge_scroll_pending
            .as_ref()
            .is_some_and(|pending| pending.direction != direction || pending.pane_id != *region.id())
        {
            self.selection_edge_scroll_acknowledged = false;
            self.selection_edge_scroll_pending = None;
        }
        let col = position
            .col
            .clamp(region.col(), self::last_region_col_saturating(region));
        let pane_id = region.id().clone();
        let previous_visible_top_row = region.visible_top_row();
        self.selection_edge_drag = Some(SelectionEdgeDrag {
            col,
            direction,
            pane_id: pane_id.clone(),
        });
        self.selection_edge_scroll_request_for(
            direction,
            pane_id,
            previous_visible_top_row,
            ClientMousePosition::new(row, col),
        )
    }

    fn refresh_edge_drag_selection(&mut self, stdout: &mut impl Write) -> rootcause::Result<()> {
        let Some(position) = self.selection_edge_drag_position() else {
            self.selection_edge_drag = None;
            return Ok(());
        };
        // Edge-drag scrolling changes the viewport before the next mouse packet arrives; refresh the drag focus after
        // the scrolled frame renders so the selected range grows with the content under the held pointer.
        self.apply_selection_input(stdout, SelectionInput::Update(position))
    }

    fn selection_edge_drag_position(&self) -> Option<ClientMousePosition> {
        let drag = self.selection_edge_drag.as_ref()?;
        let region = self::region_for_pane_id(&self.pane_regions, &drag.pane_id)?;
        Some(ClientMousePosition::new(
            self::selection_edge_row(region, drag.direction),
            drag.col.clamp(region.col(), self::last_region_col_saturating(region)),
        ))
    }

    fn selection_edge_scroll_request(&self) -> Option<SelectionEdgeScrollRequest> {
        let drag = self.selection_edge_drag.clone()?;
        let previous_visible_top_row = self::region_for_pane_id(&self.pane_regions, &drag.pane_id)?.visible_top_row();
        let position = self.selection_edge_drag_position()?;
        self.selection_edge_scroll_request_for(drag.direction, drag.pane_id, previous_visible_top_row, position)
    }

    fn selection_edge_scroll_request_for(
        &self,
        direction: PaneScrollDirection,
        pane_id: PaneId,
        previous_visible_top_row: u64,
        position: ClientMousePosition,
    ) -> Option<SelectionEdgeScrollRequest> {
        if self.selection_edge_scroll_pending.is_some() {
            return None;
        }
        Some(SelectionEdgeScrollRequest {
            pending: SelectionEdgeScrollPending {
                direction,
                pane_id,
                previous_visible_top_row,
            },
            request: ClientRequest::ScrollPaneLineAt { direction, position },
        })
    }

    fn update_selection_edge_scroll_pending(&mut self) {
        let Some(pending) = self.selection_edge_scroll_pending.as_ref() else {
            self.selection_edge_scroll_acknowledged = false;
            return;
        };
        let Some(region) = self::region_for_pane_id(&self.pane_regions, &pending.pane_id) else {
            self.selection_edge_scroll_acknowledged = false;
            self.selection_edge_scroll_pending = None;
            return;
        };
        self.selection_edge_scroll_acknowledged = match pending.direction {
            PaneScrollDirection::Down => region.visible_top_row() > pending.previous_visible_top_row,
            PaneScrollDirection::Up => region.visible_top_row() < pending.previous_visible_top_row,
        };
    }

    fn redraw_selection(
        &mut self,
        stdout: &mut impl Write,
        previous: Option<&SelectionRange>,
        next: Option<&SelectionRange>,
    ) -> rootcause::Result<()> {
        let rows = self::selection_rows(previous, next);
        let Some(changes) = self.frame_buffer.row_redraw_changes(&rows)? else {
            return Ok(());
        };
        self.draw(stdout, &changes)
    }
}

fn selection_rows(previous: Option<&SelectionRange>, next: Option<&SelectionRange>) -> Vec<u16> {
    let mut rows = BTreeSet::new();
    for selection in [previous, next].into_iter().flatten() {
        if let Some((start_row, end_row)) = selection.row_bounds() {
            for row in start_row..=end_row {
                rows.insert(row);
            }
        }
    }
    rows.into_iter().collect()
}

fn region_for_pane_id<'a>(regions: &'a PaneRegionsSnapshot, pane_id: &PaneId) -> Option<&'a PaneRegionSnapshot> {
    regions.regions().iter().find(|region| region.id() == pane_id)
}

fn clamp_mouse_position_to_region(position: ClientMousePosition, region: &PaneRegionSnapshot) -> ClientMousePosition {
    ClientMousePosition::new(
        position
            .row
            .clamp(region.row(), self::last_region_row_saturating(region)),
        position
            .col
            .clamp(region.col(), self::last_region_col_saturating(region)),
    )
}

const fn selection_edge_row(region: &PaneRegionSnapshot, direction: PaneScrollDirection) -> u16 {
    match direction {
        PaneScrollDirection::Up => region.row(),
        PaneScrollDirection::Down => self::last_region_row_saturating(region),
    }
}

const fn last_region_col_saturating(region: &PaneRegionSnapshot) -> u16 {
    region.col().saturating_add(region.cols().saturating_sub(1))
}

const fn last_region_row_saturating(region: &PaneRegionSnapshot) -> u16 {
    region.row().saturating_add(region.rows().saturating_sub(1))
}

fn mouse_event_starts_capture(event: ClientMouseEvent) -> bool {
    event.phase() == ClientMouseEventPhase::Press && event.button() & (32 | 64) == 0 && event.button() & 0b11 != 0b11
}

async fn open_session(
    session: &SessionName,
    terminal_size: TerminalSize,
    server_executable: &Path,
) -> rootcause::Result<AttachedSession> {
    let paths = SessionPaths::from_home(session)?;

    match self::attach(session, &paths, terminal_size.clone()).await {
        Ok(attached_session) => return Ok(attached_session),
        Err(attach_failure) => {
            self::handle_attach_failure(attach_failure)?;
            self::cleanup_stale_session_files(&paths)?;
        }
    }

    self::spawn_server_process(session, server_executable)?;
    self::attach_started_server(session, &paths, terminal_size).await
}

async fn attach(
    session: &SessionName,
    paths: &SessionPaths,
    terminal_size: TerminalSize,
) -> Result<AttachedSession, AttachFailure> {
    let mut connection = self::connect_with_timeout(paths).await?;

    tokio::time::timeout(
        ATTACH_TIMEOUT,
        connection.send_request(&ClientRequest::Attach(AttachRequest {
            session: session.clone(),
            terminal_size,
        })),
    )
    .await
    .map_err(|_| AttachFailure::Unusable(report!("timed out writing muxr attach request")))?
    .map_err(AttachFailure::Unusable)?;

    let (layout, pane_regions) = match tokio::time::timeout(ATTACH_TIMEOUT, connection.recv_event())
        .await
        .map_err(|_| AttachFailure::Unusable(report!("timed out waiting for muxr attach response")))?
        .map_err(AttachFailure::Unusable)?
    {
        Some(ServerEvent::Attached(attached)) => (attached.layout, attached.pane_regions),
        Some(ServerEvent::Error(error)) => {
            return Err(AttachFailure::Rejected(
                report!("muxr server rejected attach")
                    .attach(format!("code={}", error.code()))
                    .attach(format!("msg={}", error.msg())),
            ));
        }
        Some(event) => {
            return Err(AttachFailure::Unusable(
                report!("unexpected muxr server attach event").attach(format!("{event:?}")),
            ));
        }
        None => return Err(AttachFailure::Unusable(report!("muxr server closed before attach"))),
    };

    let (reader, writer) = connection.split();
    Ok(AttachedSession {
        layout,
        pane_regions,
        reader,
        writer,
    })
}

async fn connect_with_timeout(paths: &SessionPaths) -> Result<ClientConnection, AttachFailure> {
    tokio::time::timeout(ATTACH_TIMEOUT, ClientConnection::connect(&paths.socket))
        .await
        .map_err(|_| AttachFailure::Unusable(report!("timed out connecting muxr session socket")))?
        .map_err(AttachFailure::Unusable)
}

async fn attach_started_server(
    session: &SessionName,
    paths: &SessionPaths,
    terminal_size: TerminalSize,
) -> rootcause::Result<AttachedSession> {
    let started_at = Instant::now();

    loop {
        match self::attach(session, paths, terminal_size.clone()).await {
            Ok(attached_session) => return Ok(attached_session),
            Err(AttachFailure::Rejected(error)) => return Err(error),
            Err(AttachFailure::Unusable(error)) => {
                // Socket path creation can win the race against listener readiness after spawning the server.
                if started_at.elapsed() > SERVER_READY_TIMEOUT {
                    return Err(error);
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn run_interactive(mut attached_session: AttachedSession, initial_size: TerminalSize) -> rootcause::Result<()> {
    let _terminal_guard = TerminalGuard::enable_if_terminal()?;
    let (control_sender, control_receiver) = tokio::sync::mpsc::channel(CONTROL_REQUEST_CHANNEL_LIMIT);
    let (input_action_sender, mut input_action_receiver) = tokio::sync::mpsc::channel(INPUT_REQUEST_CHANNEL_LIMIT);
    let (input_request_sender, input_receiver) = tokio::sync::mpsc::channel(INPUT_REQUEST_CHANNEL_LIMIT);
    let stdin_handle = self::spawn_stdin_forwarder(input_action_sender);
    let resize_handle = self::spawn_resize_forwarder(control_sender.clone(), initial_size);
    let writer = attached_session.writer;
    let writer_handle =
        tokio::spawn(async move { self::forward_client_requests(writer, control_receiver, input_receiver).await });
    let mut stdout = std::io::stdout();
    let mut renderer = ClientRenderer::new(attached_session.layout, attached_session.pane_regions);
    renderer.sync_mouse_capture(&mut stdout)?;
    let edge_scroll_tick_start = tokio::time::Instant::now()
        .checked_add(SELECTION_EDGE_SCROLL_INTERVAL)
        .ok_or_else(|| report!("muxr selection edge scroll interval overflowed"))?;
    let mut edge_scroll_tick = tokio::time::interval_at(edge_scroll_tick_start, SELECTION_EDGE_SCROLL_INTERVAL);
    edge_scroll_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut input_actions_closed = false;

    loop {
        tokio::select! {
            event = attached_session.reader.recv_event() => {
                let Some(event) = event? else {
                    break;
                };
                match event {
                    ServerEvent::Detached => break,
                    ServerEvent::Error(error) => {
                        return Err(report!("muxr server returned error")
                            .attach(format!("code={}", error.code()))
                            .attach(format!("msg={}", error.msg())));
                    }
                    ServerEvent::Ping => {
                        if control_sender.send(ClientRequest::Pong).await.is_err() {
                            break;
                        }
                    }
                    ServerEvent::Layout(next_layout) => {
                        renderer.apply_layout(next_layout);
                    }
                    ServerEvent::PaneRegions(next_regions) => {
                        renderer.apply_pane_regions(&mut stdout, next_regions)?;
                    }
                    ServerEvent::Render(update) => match renderer.apply_render(&mut stdout, update)? {
                        ClientRenderOutcome::Drawn => {}
                        ClientRenderOutcome::NeedsResync => {
                            if control_sender.send(ClientRequest::RenderResync).await.is_err() {
                                break;
                            }
                        }
                    },
                    ServerEvent::Attached(_) | ServerEvent::Pong => {}
                }
            },
            action = input_action_receiver.recv(), if !input_actions_closed => {
                let Some(action) = action else {
                    input_actions_closed = true;
                    continue;
                };
                if !self::handle_client_input_action(action, &input_request_sender, &mut renderer, &mut stdout).await? {
                    break;
                }
            },
            _ = edge_scroll_tick.tick(), if renderer.selection_edge_drag.is_some() => {
                if !self::send_selection_edge_scroll_request(&input_request_sender, &mut renderer) {
                    break;
                }
            },
            else => {
                if input_actions_closed {
                    break;
                }
            }
        }
    }

    writer_handle.abort();
    drop(writer_handle.await);
    drop(stdin_handle);
    drop(resize_handle);
    Ok(())
}

async fn handle_client_input_action(
    action: ClientInputAction,
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    renderer: &mut ClientRenderer,
    stdout: &mut impl Write,
) -> rootcause::Result<bool> {
    match action {
        ClientInputAction::CopySelection => {
            renderer.copy_selection()?;
            Ok(true)
        }
        ClientInputAction::Mouse(event) => self::handle_mouse_input_action(event, input_sender, renderer, stdout).await,
        ClientInputAction::ServerRequest(request) => {
            if input_sender.send(request).await.is_err() {
                return Ok(false);
            }
            Ok(true)
        }
    }
}

async fn handle_mouse_input_action(
    event: ClientMouseEvent,
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    renderer: &mut ClientRenderer,
    stdout: &mut impl Write,
) -> rootcause::Result<bool> {
    let Some(position) = self::pane_position(event.position()) else {
        // Captured app drags can finish over muxr chrome; forward them clamped to the captured pane before dropping
        // ordinary chrome-row mouse packets.
        if renderer.mouse_capture.is_some()
            && let Some(event) = renderer.mouse_request_for_event(event)
        {
            return self::send_mouse_request(input_sender, event).await;
        }
        // Local selections can also finish over muxr chrome; keep update/end routed so the retained pane drag is
        // clamped and finalized instead of leaving stale drag state behind.
        match self::local_mouse_action(event) {
            Some(LocalMouseAction::SelectionUpdate(position)) => {
                let scroll_request = renderer.set_selection_edge_drag(position, Some(PaneScrollDirection::Up));
                renderer.apply_selection_input(stdout, SelectionInput::Update(position))?;
                if let Some(request) = scroll_request {
                    return Ok(self::send_edge_scroll_request(input_sender, renderer, request));
                }
            }
            Some(LocalMouseAction::SelectionEnd(position)) => {
                renderer.apply_selection_input(stdout, SelectionInput::End(position))?;
            }
            Some(LocalMouseAction::FocusAndSelectionStart(_) | LocalMouseAction::Scroll { .. }) | None => {}
        }
        return Ok(true);
    };
    let event = event.with_position(position);
    if let Some(event) = renderer.mouse_request_for_event(event) {
        return self::send_mouse_request(input_sender, event).await;
    }

    match self::local_mouse_action(event) {
        Some(LocalMouseAction::FocusAndSelectionStart(position)) => {
            if input_sender.send(ClientRequest::FocusPaneAt(position)).await.is_err() {
                return Ok(false);
            }
            renderer.apply_selection_input(stdout, SelectionInput::Start(position))?;
            Ok(true)
        }
        Some(LocalMouseAction::SelectionUpdate(position)) => {
            let scroll_request = renderer.set_selection_edge_drag(position, None);
            renderer.apply_selection_input(stdout, SelectionInput::Update(position))?;
            if let Some(request) = scroll_request {
                return Ok(self::send_edge_scroll_request(input_sender, renderer, request));
            }
            Ok(true)
        }
        Some(LocalMouseAction::SelectionEnd(position)) => {
            renderer.apply_selection_input(stdout, SelectionInput::End(position))?;
            Ok(true)
        }
        Some(LocalMouseAction::Scroll { position, direction }) => Ok(!matches!(
            self::send_droppable_request(input_sender, ClientRequest::ScrollPaneAt { position, direction }),
            DroppableSendOutcome::Closed
        )),
        None => Ok(true),
    }
}

async fn send_mouse_request(
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    event: ClientMouseEvent,
) -> rootcause::Result<bool> {
    if event.button() & (32 | 64) != 0 {
        return Ok(!matches!(
            self::send_droppable_request(input_sender, ClientRequest::Mouse(event)),
            DroppableSendOutcome::Closed
        ));
    }
    if input_sender.send(ClientRequest::Mouse(event)).await.is_err() {
        return Ok(false);
    }
    Ok(true)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DroppableSendOutcome {
    Closed,
    Dropped,
    Sent,
}

fn send_droppable_request(
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    request: ClientRequest,
) -> DroppableSendOutcome {
    match input_sender.try_send(request) {
        Ok(()) => DroppableSendOutcome::Sent,
        Err(tokio::sync::mpsc::error::TrySendError::Full(request)) => {
            drop(request);
            DroppableSendOutcome::Dropped
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(request)) => {
            drop(request);
            DroppableSendOutcome::Closed
        }
    }
}

fn send_edge_scroll_request(
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    renderer: &mut ClientRenderer,
    request: SelectionEdgeScrollRequest,
) -> bool {
    match self::send_droppable_request(input_sender, request.request) {
        DroppableSendOutcome::Sent => {
            // One queued edge-scroll request must be paired with one moved viewport and its render before another
            // request is queued; otherwise coalesced renders can skip selected content rows.
            renderer.selection_edge_scroll_acknowledged = false;
            renderer.selection_edge_scroll_pending = Some(request.pending);
            true
        }
        DroppableSendOutcome::Dropped => true,
        DroppableSendOutcome::Closed => false,
    }
}

fn send_selection_edge_scroll_request(
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    renderer: &mut ClientRenderer,
) -> bool {
    let Some(request) = renderer.selection_edge_scroll_request() else {
        return true;
    };
    self::send_edge_scroll_request(input_sender, renderer, request)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalMouseAction {
    FocusAndSelectionStart(ClientMousePosition),
    Scroll {
        position: ClientMousePosition,
        direction: PaneScrollDirection,
    },
    SelectionEnd(ClientMousePosition),
    SelectionUpdate(ClientMousePosition),
}

fn local_mouse_action(event: ClientMouseEvent) -> Option<LocalMouseAction> {
    let position = event.position();
    if event.phase() == ClientMouseEventPhase::Release {
        return Some(LocalMouseAction::SelectionEnd(position));
    }
    if event.button() & 64 != 0 {
        return match event.button() & 0b11 {
            0 => Some(LocalMouseAction::Scroll {
                position,
                direction: PaneScrollDirection::Up,
            }),
            1 => Some(LocalMouseAction::Scroll {
                position,
                direction: PaneScrollDirection::Down,
            }),
            _ => None,
        };
    }
    if event.button() & 0b11 != 0 {
        return None;
    }
    if event.button() & 32 != 0 {
        return Some(LocalMouseAction::SelectionUpdate(position));
    }
    Some(LocalMouseAction::FocusAndSelectionStart(position))
}

async fn forward_client_requests(
    mut writer: ClientRequestWriter,
    mut control_receiver: tokio::sync::mpsc::Receiver<ClientRequest>,
    mut input_receiver: tokio::sync::mpsc::Receiver<ClientRequest>,
) -> rootcause::Result<()> {
    let mut control_closed = false;
    let mut input_closed = false;

    loop {
        if control_closed && input_closed {
            break;
        }

        tokio::select! {
            biased;
            request = control_receiver.recv(), if !control_closed => match request {
                Some(request) => {
                    if writer.send_request(&request).await.is_err() {
                        break;
                    }
                }
                None => control_closed = true,
            },
            request = input_receiver.recv(), if !input_closed => match request {
                Some(request) => {
                    if writer.send_request(&request).await.is_err() {
                        break;
                    }
                }
                None => input_closed = true,
            },
        }
    }

    Ok(())
}

fn handle_attach_failure(attach_failure: AttachFailure) -> rootcause::Result<()> {
    match attach_failure {
        AttachFailure::Rejected(attach_error) => {
            // A structured muxr rejection proves the socket is live even if pid metadata is missing or stale.
            Err(attach_error).attach("socket returned a structured muxr response")
        }
        AttachFailure::Unusable(attach_error) => {
            // Even stale/incompatible servers may still answer Ping; an unusable attach is the compatibility signal.
            drop(attach_error);
            Ok(())
        }
    }
}

fn cleanup_stale_session_files(paths: &SessionPaths) -> rootcause::Result<()> {
    // Structured rejections stop before cleanup; unusable attach failures may be stale incompatible servers.
    self::remove_file_if_exists(&paths.socket)?;
    self::remove_file_if_exists(&paths.pid)?;
    Ok(())
}

fn spawn_server_process(session: &SessionName, server_executable: &Path) -> rootcause::Result<()> {
    let mut command = self::server_command(session, server_executable);

    let child = command.spawn().context("failed to spawn muxr internal server")?;
    drop(child);
    Ok(())
}

fn server_command(session: &SessionName, server_executable: &Path) -> Command {
    let mut command = Command::new(server_executable);
    command
        .arg(INTERNAL_SERVER_ARG)
        .arg(session.as_ref())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);

    command
}

fn remove_file_if_exists(path: &Path) -> rootcause::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("failed to remove stale muxr file")?,
    }
}

fn current_terminal_size() -> rootcause::Result<TerminalSize> {
    let (cols, rows) = crossterm::terminal::size().context("failed to read muxr terminal size")?;
    TerminalSize::new(cols, rows)
}

fn pane_size_for_terminal(size: &TerminalSize) -> rootcause::Result<TerminalSize> {
    let rows = size.rows().saturating_sub(TAB_BAR_ROWS).max(1);
    TerminalSize::new(size.cols(), rows)
}

fn spawn_stdin_forwarder(input_sender: tokio::sync::mpsc::Sender<ClientInputAction>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (read_sender, read_receiver) = std::sync::mpsc::channel();
        drop(self::spawn_stdin_reader(read_sender));
        let mut decoder = InputDecoder::default();

        loop {
            // Ambiguous escape prefixes need an idle timeout for bare Esc. Bracketed paste waits for its terminator so
            // slow multi-chunk paste cannot leak raw paste markers into the PTY.
            let read = if decoder.needs_idle_timeout() {
                match read_receiver.recv_timeout(AMBIGUOUS_INPUT_TIMEOUT) {
                    Ok(read) => read,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if !self::send_decoded_input(&input_sender, decoder.finalize()) {
                            break;
                        }
                        continue;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => StdinRead::Eof,
                }
            } else {
                read_receiver.recv().unwrap_or(StdinRead::Eof)
            };

            match read {
                StdinRead::Bytes(bytes) => {
                    if !self::send_decoded_input(&input_sender, decoder.decode(&bytes)) {
                        break;
                    }
                }
                StdinRead::Eof => {
                    if !self::send_decoded_input(&input_sender, decoder.finalize()) {
                        break;
                    }
                    // EOF detach follows any queued stdin bytes so piped commands like `exit\n` reach the shell first.
                    drop(input_sender.blocking_send(ClientInputAction::ServerRequest(ClientRequest::Detach)));
                    break;
                }
            }
        }
    })
}

fn spawn_stdin_reader(sender: std::sync::mpsc::Sender<StdinRead>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut buffer = [0; STDIN_BUFFER_SIZE];

        loop {
            match stdin.read(&mut buffer) {
                Ok(0) | Err(_) => {
                    drop(sender.send(StdinRead::Eof));
                    break;
                }
                Ok(bytes_read) => {
                    let Some(bytes) = buffer.get(..bytes_read) else {
                        drop(sender.send(StdinRead::Eof));
                        break;
                    };
                    if sender.send(StdinRead::Bytes(bytes.to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    })
}

fn send_decoded_input(input_sender: &tokio::sync::mpsc::Sender<ClientInputAction>, decoded: Vec<DecodedInput>) -> bool {
    for decoded in decoded {
        let action = match decoded {
            DecodedInput::CopySelection => ClientInputAction::CopySelection,
            DecodedInput::Input(bytes) => ClientInputAction::ServerRequest(ClientRequest::Input(bytes)),
            DecodedInput::Key(key) => {
                // Key events come from stdin; keep them on the input queue so raw-byte fallback cannot overtake earlier
                // PTY bytes.
                ClientInputAction::ServerRequest(ClientRequest::Key(key))
            }
            DecodedInput::Mouse(event) if self::mouse_event_can_be_dropped(event) => {
                if !self::send_droppable_input_action(input_sender, ClientInputAction::Mouse(event)) {
                    return false;
                }
                continue;
            }
            DecodedInput::Mouse(event) => ClientInputAction::Mouse(event),
            DecodedInput::Paste(bytes) => ClientInputAction::ServerRequest(ClientRequest::Paste(bytes)),
        };
        if input_sender.blocking_send(action).is_err() {
            return false;
        }
    }

    true
}

fn send_droppable_input_action(
    input_sender: &tokio::sync::mpsc::Sender<ClientInputAction>,
    action: ClientInputAction,
) -> bool {
    match input_sender.try_send(action) {
        Ok(()) => true,
        Err(tokio::sync::mpsc::error::TrySendError::Full(action)) => {
            drop(action);
            true
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(action)) => {
            drop(action);
            false
        }
    }
}

const fn mouse_event_can_be_dropped(event: ClientMouseEvent) -> bool {
    event.button() & (32 | 64) != 0
}

fn pane_position(position: muxr_core::ClientMousePosition) -> Option<muxr_core::ClientMousePosition> {
    Some(muxr_core::ClientMousePosition::new(
        position.row.checked_sub(TAB_BAR_ROWS)?,
        position.col,
    ))
}

fn spawn_resize_forwarder(
    sender: tokio::sync::mpsc::Sender<ClientRequest>,
    initial_size: TerminalSize,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut last_size = initial_size;

        loop {
            if sender.is_closed() {
                break;
            }

            thread::sleep(RESIZE_POLL_INTERVAL);
            let Ok(next_terminal_size) = self::current_terminal_size() else {
                break;
            };
            // Resize requests use the pane viewport, because the first host-terminal row is reserved for the tab bar.
            let Ok(next_size) = self::pane_size_for_terminal(&next_terminal_size) else {
                break;
            };
            if next_size == last_size {
                continue;
            }

            if sender.blocking_send(ClientRequest::Resize(next_size.clone())).is_err() {
                break;
            }
            last_size = next_size;
        }
    })
}

fn run_async<T>(future: impl Future<Output = rootcause::Result<T>>) -> rootcause::Result<T> {
    tokio::runtime::Runtime::new()
        .context("failed to build muxr tokio runtime")?
        .block_on(future)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::io::Write;
    use std::path::Path;

    use muxr_core::ClientKey;
    use muxr_core::ClientKeyCode;
    use muxr_core::ClientKeyModifiers;
    use muxr_core::LayoutSnapshot;
    use muxr_core::PaneId;
    use muxr_core::PaneSnapshot;
    use muxr_core::ServerError;
    use muxr_core::TabId;
    use muxr_core::TabSnapshot;
    use muxr_transport::ServerListener;

    use super::*;

    #[test]
    fn test_cleanup_stale_session_files_when_running_pid_has_missing_socket_removes_pid() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (_, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            fs::write(&paths.pid, std::process::id().to_string())?;

            handle_attach_failure(AttachFailure::Unusable(report!("connect failed")))?;
            cleanup_stale_session_files(&paths)?;

            assert2::assert!(!paths.pid.exists());
            assert2::assert!(!paths.socket.exists());
            Ok(())
        })
    }

    #[test]
    fn test_handle_attach_failure_when_server_rejects_and_pid_is_missing_returns_error() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (_, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let _listener = ServerListener::bind(&paths.socket)?;

            assert2::assert!(handle_attach_failure(AttachFailure::Rejected(report!("already attached"))).is_err());
            assert2::assert!(paths.socket.exists());
            Ok(())
        })
    }

    #[test]
    fn test_cleanup_stale_session_files_when_attach_is_unusable_removes_live_socket_path() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (_, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let _listener = ServerListener::bind(&paths.socket)?;

            handle_attach_failure(AttachFailure::Unusable(report!("failed to attach")))?;
            cleanup_stale_session_files(&paths)?;

            assert2::assert!(!paths.socket.exists());
            Ok(())
        })
    }

    #[test]
    fn test_server_command_uses_supplied_executable_for_internal_server() -> rootcause::Result<()> {
        let session: SessionName = "work".parse()?;
        let command = server_command(&session, Path::new("/tmp/custom-muxr"));
        let args: Vec<_> = command.get_args().collect();

        pretty_assertions::assert_eq!(command.get_program(), OsStr::new("/tmp/custom-muxr"));
        pretty_assertions::assert_eq!(args.as_slice(), [OsStr::new(INTERNAL_SERVER_ARG), OsStr::new("work")]);
        Ok(())
    }

    #[test]
    fn test_attach_when_server_rejects_returns_rejected_error() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let listener = ServerListener::bind(&paths.socket)?;
            let handle = tokio::spawn(async move {
                let mut connection = listener.accept().await?;
                assert2::assert!(matches!(
                    connection.recv_request().await?,
                    Some(ClientRequest::Attach(_))
                ));
                connection
                    .send_event(&ServerEvent::Error(ServerError::ClientAlreadyAttached))
                    .await?;
                Ok::<(), rootcause::Report>(())
            });

            let attach_error = attach(&session, &paths, TerminalSize::new(80, 24)?).await.map_or_else(
                |failure| match failure {
                    AttachFailure::Rejected(error) | AttachFailure::Unusable(error) => error,
                },
                |_| report!("expected rejected attach"),
            );

            assert2::assert!(attach_error.to_string().contains("muxr server rejected attach"));
            handle
                .await
                .map_err(|error| report!("muxr rejected attach test task panicked").attach(format!("{error}")))??;
            Ok(())
        })
    }

    #[test]
    fn test_forward_client_requests_when_input_queue_is_ready_sends_control_first() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (_, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let listener = ServerListener::bind(&paths.socket)?;
            let server_handle = tokio::spawn(async move {
                let mut connection = listener.accept().await?;
                let Some(request) = connection.recv_request().await? else {
                    return Err(report!("expected forwarded client request"));
                };
                Ok::<ClientRequest, rootcause::Report>(request)
            });

            let connection = ClientConnection::connect(&paths.socket).await?;
            let (_reader, writer) = connection.split();
            let (control_sender, control_receiver) = tokio::sync::mpsc::channel(1);
            let (input_sender, input_receiver) = tokio::sync::mpsc::channel(1);
            assert2::assert!(input_sender.try_send(ClientRequest::Input(vec![b'a'])).is_ok());
            assert2::assert!(input_sender.try_send(ClientRequest::Input(vec![b'b'])).is_err());
            assert2::assert!(control_sender.try_send(ClientRequest::Pong).is_ok());

            let writer_handle = tokio::spawn(self::forward_client_requests(writer, control_receiver, input_receiver));
            let first_request = server_handle
                .await
                .map_err(|error| report!("muxr forward test socket task panicked").attach(format!("{error}")))??;

            pretty_assertions::assert_eq!(first_request, ClientRequest::Pong);
            drop(control_sender);
            drop(input_sender);
            writer_handle
                .await
                .map_err(|error| report!("muxr forward test writer task panicked").attach(format!("{error}")))??;
            Ok(())
        })
    }

    #[test]
    fn test_forward_client_requests_when_stdin_requests_are_mixed_sends_input_queue_in_order() -> rootcause::Result<()>
    {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (_, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let listener = ServerListener::bind(&paths.socket)?;
            let server_handle = tokio::spawn(async move {
                let mut connection = listener.accept().await?;
                let mut requests = Vec::new();
                for _ in 0..3 {
                    let Some(request) = connection.recv_request().await? else {
                        return Err(report!("expected forwarded stdin request"));
                    };
                    requests.push(request);
                }
                Ok::<Vec<ClientRequest>, rootcause::Report>(requests)
            });

            let connection = ClientConnection::connect(&paths.socket).await?;
            let (_reader, writer) = connection.split();
            let (control_sender, control_receiver) = tokio::sync::mpsc::channel(1);
            let (input_sender, input_receiver) = tokio::sync::mpsc::channel(3);
            let key = ClientKey::new(
                ClientKeyCode::Char('E'),
                ClientKeyModifiers::SHIFT_ALT,
                b"\x1bE".to_vec(),
            );
            assert2::assert!(input_sender.try_send(ClientRequest::Input(b"a".to_vec())).is_ok());
            assert2::assert!(input_sender.try_send(ClientRequest::Key(key.clone())).is_ok());
            assert2::assert!(input_sender.try_send(ClientRequest::Input(b"b".to_vec())).is_ok());
            drop(control_sender);
            drop(input_sender);

            let writer_handle = tokio::spawn(self::forward_client_requests(writer, control_receiver, input_receiver));
            let requests = server_handle.await.map_err(|error| {
                report!("muxr forward order test socket task panicked").attach(format!("{error}"))
            })??;

            pretty_assertions::assert_eq!(
                requests,
                vec![
                    ClientRequest::Input(b"a".to_vec()),
                    ClientRequest::Key(key),
                    ClientRequest::Input(b"b".to_vec()),
                ],
            );
            writer_handle.await.map_err(|error| {
                report!("muxr forward order test writer task panicked").attach(format!("{error}"))
            })??;
            Ok(())
        })
    }

    #[test]
    fn test_forward_client_requests_when_stdin_detach_follows_input_sends_input_before_detach() -> rootcause::Result<()>
    {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (_, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let listener = ServerListener::bind(&paths.socket)?;
            let server_handle = tokio::spawn(async move {
                let mut connection = listener.accept().await?;
                let mut requests = Vec::new();
                for _ in 0..2 {
                    let Some(request) = connection.recv_request().await? else {
                        return Err(report!("expected forwarded stdin detach request"));
                    };
                    requests.push(request);
                }
                Ok::<Vec<ClientRequest>, rootcause::Report>(requests)
            });

            let connection = ClientConnection::connect(&paths.socket).await?;
            let (_reader, writer) = connection.split();
            let (control_sender, control_receiver) = tokio::sync::mpsc::channel(1);
            let (input_sender, input_receiver) = tokio::sync::mpsc::channel(2);
            assert2::assert!(input_sender.try_send(ClientRequest::Input(b"exit\n".to_vec())).is_ok());
            assert2::assert!(input_sender.try_send(ClientRequest::Detach).is_ok());
            drop(control_sender);
            drop(input_sender);

            let writer_handle = tokio::spawn(self::forward_client_requests(writer, control_receiver, input_receiver));
            let requests = server_handle
                .await
                .map_err(|error| report!("muxr forward EOF test socket task panicked").attach(format!("{error}")))??;

            pretty_assertions::assert_eq!(
                requests,
                vec![ClientRequest::Input(b"exit\n".to_vec()), ClientRequest::Detach],
            );
            writer_handle
                .await
                .map_err(|error| report!("muxr forward EOF test writer task panicked").attach(format!("{error}")))??;
            Ok(())
        })
    }

    #[test]
    fn test_send_decoded_input_when_key_arrives_uses_input_queue_in_order() {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(3);
        let key = ClientKey::new(
            ClientKeyCode::Char('E'),
            ClientKeyModifiers::SHIFT_ALT,
            b"\x1bE".to_vec(),
        );

        assert2::assert!(send_decoded_input(
            &input_sender,
            vec![
                DecodedInput::Input(b"a".to_vec()),
                DecodedInput::Key(key.clone()),
                DecodedInput::Input(b"b".to_vec()),
            ],
        ));

        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientInputAction::ServerRequest(ClientRequest::Input(b"a".to_vec()))),
        );
        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientInputAction::ServerRequest(ClientRequest::Key(key))),
        );
        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientInputAction::ServerRequest(ClientRequest::Input(b"b".to_vec()))),
        );
    }

    #[test]
    fn test_send_decoded_input_when_paste_arrives_uses_input_queue() {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);

        assert2::assert!(send_decoded_input(
            &input_sender,
            vec![DecodedInput::Paste(b"one\ntwo\n".to_vec())],
        ));

        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientInputAction::ServerRequest(ClientRequest::Paste(
                b"one\ntwo\n".to_vec()
            ))),
        );
    }

    #[test]
    fn test_send_decoded_input_when_mouse_arrives_emits_local_mouse_action() {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
        let event = ClientMouseEvent::new(
            0,
            ClientMouseEventPhase::Press,
            muxr_core::ClientMousePosition::new(4, 9),
        );

        assert2::assert!(send_decoded_input(&input_sender, vec![DecodedInput::Mouse(event)]));

        pretty_assertions::assert_eq!(input_receiver.blocking_recv(), Some(ClientInputAction::Mouse(event)),);
    }

    #[test]
    fn test_send_decoded_input_when_mouse_motion_action_queue_is_full_drops_without_blocking() -> rootcause::Result<()>
    {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
        assert2::assert!(input_sender.try_send(ClientInputAction::CopySelection).is_ok());
        let event = ClientMouseEvent::new(
            32,
            ClientMouseEventPhase::Press,
            muxr_core::ClientMousePosition::new(4, 9),
        );
        let (result_sender, result_receiver) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            let _ = result_sender.send(send_decoded_input(&input_sender, vec![DecodedInput::Mouse(event)]));
        });
        let result = match result_receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(result) => result,
            Err(error) => {
                drop(input_receiver);
                handle
                    .join()
                    .map_err(|error| report!("muxr mouse input test thread panicked").attach(format!("{error:?}")))?;
                return Err(report!("muxr mouse motion blocked on full input-action queue").attach(format!("{error}")));
            }
        };

        assert2::assert!(result);
        pretty_assertions::assert_eq!(input_receiver.try_recv(), Ok(ClientInputAction::CopySelection));
        assert2::assert!(input_receiver.try_recv().is_err());
        handle
            .join()
            .map_err(|error| report!("muxr mouse input test thread panicked").attach(format!("{error:?}")))?;
        Ok(())
    }

    #[test]
    fn test_send_decoded_input_when_copy_selection_arrives_emits_local_action() {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);

        assert2::assert!(send_decoded_input(&input_sender, vec![DecodedInput::CopySelection]));

        pretty_assertions::assert_eq!(input_receiver.blocking_recv(), Some(ClientInputAction::CopySelection));
    }

    #[test]
    fn test_handle_client_input_action_when_plain_mouse_click_arrives_focuses_pane() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
            let mut renderer = ClientRenderer::with_synchronized_output(
                layout_snapshot()?,
                pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();

            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent::new(
                        0,
                        ClientMouseEventPhase::Press,
                        muxr_core::ClientMousePosition::new(1, 1),
                    )),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::FocusPaneAt(muxr_core::ClientMousePosition::new(0, 1))),
            );
            Ok(())
        })
    }

    #[test]
    fn test_handle_client_input_action_when_selection_release_is_on_tab_bar_finalizes_selection()
    -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
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

            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent::new(
                        0,
                        ClientMouseEventPhase::Press,
                        muxr_core::ClientMousePosition::new(1, 0),
                    )),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent::new(
                        0,
                        ClientMouseEventPhase::Release,
                        muxr_core::ClientMousePosition::new(0, 1),
                    )),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::FocusPaneAt(muxr_core::ClientMousePosition::new(0, 0))),
            );
            pretty_assertions::assert_eq!(renderer.selection.selected_text(), Some("ab".to_owned()),);
            Ok(())
        })
    }

    #[test]
    fn test_handle_client_input_action_when_selection_drag_moves_above_pane_scrolls_up() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(2);
            let mut renderer = ClientRenderer::with_synchronized_output(
                layout_snapshot()?,
                pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();

            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent::new(
                        0,
                        ClientMouseEventPhase::Press,
                        muxr_core::ClientMousePosition::new(1, 0),
                    )),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent::new(
                        32,
                        ClientMouseEventPhase::Press,
                        muxr_core::ClientMousePosition::new(0, 0),
                    )),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::FocusPaneAt(muxr_core::ClientMousePosition::new(0, 0))),
            );
            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::ScrollPaneLineAt {
                    direction: PaneScrollDirection::Up,
                    position: muxr_core::ClientMousePosition::new(0, 0),
                }),
            );
            Ok(())
        })
    }

    #[test]
    fn test_send_selection_edge_scroll_request_when_scroll_is_pending_waits_for_render_ack() -> rootcause::Result<()> {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(2);
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let mut output = CountingWriter::default();
        renderer.apply_render(&mut output, muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        renderer.apply_selection_input(&mut output, SelectionInput::Start(ClientMousePosition::new(0, 0)))?;
        let initial = renderer
            .set_selection_edge_drag(ClientMousePosition::new(2, 1), None)
            .ok_or_else(|| report!("expected initial muxr edge scroll request"))?;
        let expected = ClientRequest::ScrollPaneLineAt {
            direction: PaneScrollDirection::Down,
            position: ClientMousePosition::new(0, 1),
        };
        pretty_assertions::assert_eq!(initial.request, expected);
        assert2::assert!(send_edge_scroll_request(&input_sender, &mut renderer, initial));
        pretty_assertions::assert_eq!(input_receiver.blocking_recv(), Some(expected.clone()));
        assert2::assert!(send_selection_edge_scroll_request(&input_sender, &mut renderer));
        assert2::assert!(matches!(
            input_receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        renderer.apply_pane_regions(&mut output, pane_regions_snapshot_with_visible_top_row(1)?)?;
        renderer.apply_render(&mut output, muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        assert2::assert!(send_selection_edge_scroll_request(&input_sender, &mut renderer));

        pretty_assertions::assert_eq!(input_receiver.blocking_recv(), Some(expected));
        Ok(())
    }

    #[test]
    fn test_send_edge_scroll_request_when_queue_is_full_does_not_mark_scroll_pending() -> rootcause::Result<()> {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
        assert2::assert!(input_sender.try_send(ClientRequest::Pong).is_ok());
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let mut output = CountingWriter::default();
        renderer.apply_render(&mut output, muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        renderer.apply_selection_input(&mut output, SelectionInput::Start(ClientMousePosition::new(0, 0)))?;
        let request = renderer
            .set_selection_edge_drag(ClientMousePosition::new(2, 1), None)
            .ok_or_else(|| report!("expected muxr edge scroll request"))?;

        assert2::assert!(send_edge_scroll_request(&input_sender, &mut renderer, request));
        pretty_assertions::assert_eq!(input_receiver.try_recv(), Ok(ClientRequest::Pong));
        assert2::assert!(send_selection_edge_scroll_request(&input_sender, &mut renderer));

        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientRequest::ScrollPaneLineAt {
                direction: PaneScrollDirection::Down,
                position: ClientMousePosition::new(0, 1),
            }),
        );
        Ok(())
    }

    #[test]
    fn test_handle_client_input_action_when_pane_tracks_mouse_forwards_mouse_to_server() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
            let mut renderer = ClientRenderer::with_synchronized_output(
                layout_snapshot()?,
                mouse_tracking_pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();

            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent::new(
                        0,
                        ClientMouseEventPhase::Press,
                        muxr_core::ClientMousePosition::new(1, 1),
                    )),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::Mouse(ClientMouseEvent::new(
                    0,
                    ClientMouseEventPhase::Press,
                    muxr_core::ClientMousePosition::new(0, 1),
                ))),
            );
            pretty_assertions::assert_eq!(output.flushes, 0);
            Ok(())
        })
    }

    #[test]
    fn test_handle_client_input_action_when_tracking_drag_crosses_pane_routes_to_pressed_pane() -> rootcause::Result<()>
    {
        self::runtime()?.block_on(async {
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(4);
            let mut renderer = ClientRenderer::with_synchronized_output(
                layout_snapshot()?,
                split_mouse_tracking_pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();

            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent::new(
                        0,
                        ClientMouseEventPhase::Press,
                        muxr_core::ClientMousePosition::new(1, 1),
                    )),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent::new(
                        32,
                        ClientMouseEventPhase::Press,
                        muxr_core::ClientMousePosition::new(1, 3),
                    )),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent::new(
                        0,
                        ClientMouseEventPhase::Release,
                        muxr_core::ClientMousePosition::new(0, 3),
                    )),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent::new(
                        32,
                        ClientMouseEventPhase::Press,
                        muxr_core::ClientMousePosition::new(1, 3),
                    )),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::Mouse(ClientMouseEvent::new(
                    0,
                    ClientMouseEventPhase::Press,
                    muxr_core::ClientMousePosition::new(0, 1),
                ))),
            );
            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::Mouse(ClientMouseEvent::new(
                    32,
                    ClientMouseEventPhase::Press,
                    muxr_core::ClientMousePosition::new(0, 1),
                ))),
            );
            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::Mouse(ClientMouseEvent::new(
                    0,
                    ClientMouseEventPhase::Release,
                    muxr_core::ClientMousePosition::new(0, 1),
                ))),
            );
            assert2::assert!(matches!(
                input_receiver.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ));
            Ok(())
        })
    }

    #[test]
    fn test_pane_size_for_terminal_when_tab_bar_has_room_reserves_one_row() -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(
            pane_size_for_terminal(&TerminalSize::new(80, 24)?)?,
            TerminalSize::new(80, 23)?,
        );
        pretty_assertions::assert_eq!(
            pane_size_for_terminal(&TerminalSize::new(80, 1)?)?,
            TerminalSize::new(80, 1)?,
        );
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_layout_when_no_render_arrives_writes_nothing() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(
            layout_snapshot()?,
            pane_regions_snapshot()?,
            SynchronizedOutput::Csi,
        );
        let output = CountingWriter::default();

        renderer.apply_layout(two_tab_layout()?);

        pretty_assertions::assert_eq!(output.bytes, Vec::<u8>::new());
        pretty_assertions::assert_eq!(output.flushes, 0);
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

        pretty_assertions::assert_eq!(outcome, ClientRenderOutcome::Drawn);
        pretty_assertions::assert_eq!(output.flushes, 1);
        let terminal_output = output.rendered_string()?;
        assert2::assert!(terminal_output.starts_with("\x1b[?2026h"));
        assert2::assert!(terminal_output.ends_with("\x1b[?2026l"));
        let clear_index = terminal_output.find("\x1b[2J").unwrap_or(usize::MAX);
        let tab_bar_index = terminal_output.find("[2:tab 2]").unwrap_or(usize::MAX);
        let pane_index = terminal_output.find("ab").unwrap_or(usize::MAX);
        assert2::assert!(clear_index < tab_bar_index);
        assert2::assert!(tab_bar_index < pane_index);
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

        pretty_assertions::assert_eq!(outcome, ClientRenderOutcome::NeedsResync);
        pretty_assertions::assert_eq!(output.bytes, Vec::<u8>::new());
        pretty_assertions::assert_eq!(output.flushes, 0);
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

        pretty_assertions::assert_eq!(output.rendered_string()?, "\x1b[?1003h");
        pretty_assertions::assert_eq!(output.flushes, 1);
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

        pretty_assertions::assert_eq!(output.rendered_string()?, "\x1b[?1003h\x1b[?1003l");
        pretty_assertions::assert_eq!(output.flushes, 2);
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
            SelectionInput::Start(muxr_core::ClientMousePosition::new(0, 0)),
        )?;
        renderer.apply_selection_input(
            &mut output,
            SelectionInput::Update(muxr_core::ClientMousePosition::new(0, 1)),
        )?;

        let selection_output = output.rendered_string()?;
        assert2::assert!(selection_output.contains("\x1b[7m"));
        pretty_assertions::assert_eq!(output.flushes, 1);
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
            SelectionInput::Start(muxr_core::ClientMousePosition::new(0, 0)),
        )?;
        renderer.apply_selection_input(
            &mut initial_output,
            SelectionInput::End(muxr_core::ClientMousePosition::new(0, 1)),
        )?;
        let mut output = CountingWriter::default();

        renderer.apply_pane_regions(&mut output, pane_regions_snapshot_with_visible_top_row(1)?)?;

        let redrawn = output.rendered_string()?;
        assert2::assert!(redrawn.contains("ab"));
        assert2::assert!(!redrawn.contains("\x1b[7m"));
        pretty_assertions::assert_eq!(output.flushes, 1);
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
        renderer.apply_selection_input(&mut output, SelectionInput::Start(ClientMousePosition::new(0, 0)))?;
        let scroll_request = renderer
            .set_selection_edge_drag(ClientMousePosition::new(3, 1), None)
            .map(|request| request.request);
        renderer.apply_selection_input(&mut output, SelectionInput::Update(ClientMousePosition::new(3, 1)))?;

        pretty_assertions::assert_eq!(
            scroll_request,
            Some(ClientRequest::ScrollPaneLineAt {
                direction: PaneScrollDirection::Down,
                position: ClientMousePosition::new(2, 1),
            }),
        );

        renderer.apply_pane_regions(&mut output, three_row_pane_regions_snapshot(10)?)?;
        renderer.apply_render(
            &mut output,
            muxr_core::RenderUpdate::Baseline(three_row_render_baseline("bb", "cc", "dd")?),
        )?;

        pretty_assertions::assert_eq!(renderer.selection.selected_text(), Some("aa\nbb\ncc\ndd".to_owned()),);
        let range = renderer
            .selection
            .range()
            .ok_or_else(|| report!("expected muxr edge-drag selection range"))?;
        assert2::assert!(range.contains(2, 0));
        Ok(())
    }

    #[rstest::rstest]
    #[case::same_cell_within_threshold(0, 0, 399, true)]
    #[case::same_cell_after_threshold(0, 0, 401, false)]
    #[case::different_cell_within_threshold(0, 1, 100, false)]
    fn test_click_tracker_record_when_clicks_are_repeated_detects_double_click(
        #[case] row: u16,
        #[case] col: u16,
        #[case] elapsed_ms: u64,
        #[case] expected_double: bool,
    ) -> rootcause::Result<()> {
        let mut clicks = ClickTracker::default();
        let now = Instant::now();
        pretty_assertions::assert_eq!(clicks.record(click_target(0, 0)?, now), ClickKind::Other,);

        let expected = if expected_double {
            ClickKind::Double
        } else {
            ClickKind::Other
        };
        let next_click_at = now
            .checked_add(Duration::from_millis(elapsed_ms))
            .ok_or_else(|| report!("muxr click tracker test instant overflowed"))?;
        pretty_assertions::assert_eq!(clicks.record(click_target(row, col)?, next_click_at), expected,);
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
        let first_position = ClientMousePosition::new(0, first_col);
        let second_position = ClientMousePosition::new(0, second_col);
        let second_click_at = now
            .checked_add(Duration::from_millis(100))
            .ok_or_else(|| report!("muxr double-click selection test instant overflowed"))?;
        let mut output = CountingWriter::default();

        renderer.apply_selection_input_at(&mut output, SelectionInput::Start(first_position), now)?;
        renderer.apply_selection_input_at(&mut output, SelectionInput::End(first_position), now)?;
        renderer.apply_selection_input_at(&mut output, SelectionInput::Start(second_position), second_click_at)?;

        pretty_assertions::assert_eq!(renderer.selection.selected_text(), Some("two".to_owned()),);
        assert2::assert!(output.rendered_string()?.contains("\x1b[7m"));
        pretty_assertions::assert_eq!(output.flushes, 1);
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
        let position = ClientMousePosition::new(0, 4);
        let second_click_at = now
            .checked_add(Duration::from_millis(100))
            .ok_or_else(|| report!("muxr retained double-click selection test instant overflowed"))?;
        let mut output = CountingWriter::default();

        renderer.apply_selection_input_at(&mut output, SelectionInput::Start(position), now)?;
        renderer.apply_selection_input_at(&mut output, SelectionInput::End(position), now)?;
        renderer.apply_pane_regions(&mut output, word_pane_regions_snapshot()?)?;
        renderer.apply_selection_input_at(&mut output, SelectionInput::Start(position), second_click_at)?;

        pretty_assertions::assert_eq!(renderer.selection.selected_text(), Some("two".to_owned()),);
        Ok(())
    }

    fn session_paths(base: &Path, raw: &str) -> rootcause::Result<(SessionName, SessionPaths)> {
        let session = raw.parse()?;
        let root = base.join("sessions").join(raw);

        Ok((
            session,
            SessionPaths {
                socket: root.join("server.sock"),
                pid: root.join("server.pid"),
                layout: root.join("layout.json"),
                panes: root.join("panes"),
                root,
            },
        ))
    }

    fn click_target(row: u16, col: u16) -> rootcause::Result<ClickTarget> {
        Ok(ClickTarget::Cell {
            pane_id: muxr_core::PaneId::new("pane-1")?,
            position: muxr_core::ClientMousePosition::new(row, col),
        })
    }

    async fn recv_client_request(
        input_receiver: &mut tokio::sync::mpsc::Receiver<ClientRequest>,
    ) -> rootcause::Result<Option<ClientRequest>> {
        Ok(tokio::time::timeout(Duration::from_secs(1), input_receiver.recv())
            .await
            .context("timed out waiting for muxr client request")?)
    }

    fn layout_snapshot() -> rootcause::Result<LayoutSnapshot> {
        let active_tab = TabId::new("tab-1")?;
        let active_pane = PaneId::new("pane-1")?;
        let pane = PaneSnapshot::new(active_pane.clone(), "shell");
        let tab = TabSnapshot::new(active_tab.clone(), "default", active_pane, vec![pane])?;
        LayoutSnapshot::new(active_tab, vec![tab])
    }

    fn pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        self::pane_regions_snapshot_with_visible_top_row(0)
    }

    fn pane_regions_snapshot_with_visible_top_row(visible_top_row: u64) -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![muxr_core::PaneRegionSnapshot::new(
            muxr_core::PaneId::new("pane-1")?,
            0,
            0,
            2,
            1,
            muxr_core::PaneMouseMode::None,
            visible_top_row,
        )?])
    }

    fn mouse_tracking_pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![muxr_core::PaneRegionSnapshot::new(
            muxr_core::PaneId::new("pane-1")?,
            0,
            0,
            2,
            1,
            muxr_core::PaneMouseMode::ButtonMotion,
            0,
        )?])
    }

    fn split_mouse_tracking_pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![
            muxr_core::PaneRegionSnapshot::new(
                muxr_core::PaneId::new("pane-1")?,
                0,
                0,
                2,
                1,
                muxr_core::PaneMouseMode::ButtonMotion,
                0,
            )?,
            muxr_core::PaneRegionSnapshot::new(
                muxr_core::PaneId::new("pane-2")?,
                2,
                0,
                2,
                1,
                muxr_core::PaneMouseMode::None,
                0,
            )?,
        ])
    }

    fn any_motion_pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![muxr_core::PaneRegionSnapshot::new(
            muxr_core::PaneId::new("pane-1")?,
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
            muxr_core::PaneId::new("pane-1")?,
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
            muxr_core::PaneId::new("pane-1")?,
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
            muxr_core::TabId::new("tab-2")?,
            vec![
                muxr_core::TabSnapshot::new(
                    muxr_core::TabId::new("tab-1")?,
                    "default",
                    muxr_core::PaneId::new("pane-1")?,
                    vec![muxr_core::PaneSnapshot::new(muxr_core::PaneId::new("pane-1")?, "shell")],
                )?,
                muxr_core::TabSnapshot::new(
                    muxr_core::TabId::new("tab-2")?,
                    "tab 2",
                    muxr_core::PaneId::new("pane-2")?,
                    vec![muxr_core::PaneSnapshot::new(muxr_core::PaneId::new("pane-2")?, "shell")],
                )?,
            ],
        )
    }

    fn render_baseline() -> rootcause::Result<muxr_core::RenderBaseline> {
        muxr_core::RenderBaseline::new(
            1,
            TerminalSize::new(2, 1)?,
            muxr_core::RenderCursor::new(0, 1, true),
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
            muxr_core::RenderCursor::new(0, 1, true),
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
            muxr_core::RenderCursor::new(0, 1, true),
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
            muxr_core::RenderCursor::new(0, 1, true),
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

    fn runtime() -> rootcause::Result<tokio::runtime::Runtime> {
        Ok(tokio::runtime::Runtime::new().context("failed to build muxr client test runtime")?)
    }
}
