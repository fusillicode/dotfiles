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
use muxr_core::ClientRequest;
use muxr_core::INTERNAL_SERVER_ARG;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::ServerEvent;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use muxr_core::TerminalSize;
use muxr_transport::ClientConnection;
use muxr_transport::ClientEventReader;
use muxr_transport::ClientRequestWriter;
use rootcause::prelude::ResultExt;
use rootcause::report;

use self::copy_selection::SelectionInput;
use self::pane_focus::LocalMouseAction;
use self::renderer::ClientRenderOutcome;
use self::renderer::ClientRenderer;
use self::renderer::SelectionEdgeScrollRequest;
use crate::input::DecodedInput;
use crate::input::InputDecoder;

pub mod copy_selection;
mod pane_focus;
mod pane_scroll;
mod renderer;
mod tab_bar;

const RESIZE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const ATTACH_TIMEOUT: Duration = Duration::from_secs(2);
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(2);
const AMBIGUOUS_INPUT_TIMEOUT: Duration = Duration::from_millis(50);
const DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(400);
const SELECTION_EDGE_SCROLL_INTERVAL: Duration = Duration::from_millis(50);
const STDIN_BUFFER_SIZE: usize = 8192;
const TAB_BAR_COLS: u16 = self::tab_bar::WIDTH;
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
                    ServerEvent::Deleted | ServerEvent::Detached => break,
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
                    ServerEvent::SidebarLayout(next_layout) => {
                        renderer.apply_sidebar_layout(&mut stdout, next_layout)?;
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
            _ = edge_scroll_tick.tick(), if renderer.selection_edge_drag_active() => {
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
    let Some(position) = self::pane_position(event.position) else {
        if let Some(request) = self::tab_focus_request_for_sidebar_click(event, renderer) {
            if input_sender.send(request).await.is_err() {
                return Ok(false);
            }
            return Ok(true);
        }
        // Captured app drags can finish over the tab bar; forward them clamped to the captured pane before dropping
        // ordinary tab bar mouse packets.
        if renderer.has_mouse_capture()
            && let Some(position) = self::pane_position_for_sidebar_drag(event.position)
            && let Some(event) = renderer.mouse_request_for_event(ClientMouseEvent { position, ..event })
        {
            return self::send_mouse_request(input_sender, event).await;
        }
        // Local selections can also finish over the tab bar; keep update/end routed so the retained pane drag is
        // clamped and finalized instead of leaving stale drag state behind.
        let tab_bar_position = event.position;
        match self::pane_focus::local_mouse_action(event) {
            Some(LocalMouseAction::SelectionUpdate(_)) => {
                if let Some(position) = self::pane_position_for_sidebar_drag(tab_bar_position) {
                    let scroll_request = renderer.set_selection_edge_drag(position, None);
                    renderer.apply_selection_input(stdout, SelectionInput::Update(position))?;
                    if let Some(request) = scroll_request {
                        return Ok(self::send_edge_scroll_request(input_sender, renderer, request));
                    }
                }
            }
            Some(LocalMouseAction::SelectionEnd(_)) => {
                if let Some(position) = self::pane_position_for_sidebar_drag(tab_bar_position) {
                    renderer.apply_selection_input(stdout, SelectionInput::End(position))?;
                }
            }
            Some(LocalMouseAction::FocusAndSelectionStart(_)) | None => {}
        }
        return Ok(true);
    };
    let event = ClientMouseEvent { position, ..event };
    if self::pane_scroll::is_wheel_event(event) {
        return self::send_mouse_request(input_sender, event).await;
    }
    if let Some(event) = renderer.mouse_request_for_event(event) {
        return self::send_mouse_request(input_sender, event).await;
    }

    match self::pane_focus::local_mouse_action(event) {
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
        None => Ok(true),
    }
}

async fn send_mouse_request(
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    event: ClientMouseEvent,
) -> rootcause::Result<bool> {
    if self::mouse_event_can_be_dropped(event) {
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
    let (pending, request) = request.into_parts();
    match self::send_droppable_request(input_sender, request) {
        DroppableSendOutcome::Sent => {
            // One queued edge-scroll request must be paired with one moved viewport and its render before another
            // request is queued; otherwise coalesced renders can skip selected content rows.
            renderer.mark_selection_edge_scroll_sent(pending);
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
    let mut cmd = self::server_cmd(session, server_executable);

    let child = cmd.spawn().context("failed to spawn muxr internal server")?;
    drop(child);
    Ok(())
}

fn server_cmd(session: &SessionName, server_executable: &Path) -> Command {
    let mut cmd = Command::new(server_executable);
    cmd.arg(INTERNAL_SERVER_ARG)
        .arg(session.as_ref())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);

    cmd
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
    let cols = size.cols().saturating_sub(TAB_BAR_COLS).max(1);
    TerminalSize::new(cols, size.rows())
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
                    // EOF detach follows any queued stdin bytes so piped cmds like `exit\n` reach the shell first.
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
    event.button & 32 != 0 && !crate::client::pane_scroll::is_wheel_event(event)
}

fn tab_focus_request_for_sidebar_click(event: ClientMouseEvent, renderer: &ClientRenderer) -> Option<ClientRequest> {
    if event.phase != muxr_core::ClientMouseEventPhase::Press || event.button != 0 {
        return None;
    }
    renderer
        .tab_id_at_sidebar_row(event.position.row)
        .map(ClientRequest::FocusTab)
}

fn pane_position(position: muxr_core::ClientMousePosition) -> Option<muxr_core::ClientMousePosition> {
    Some(muxr_core::ClientMousePosition {
        row: position.row,
        col: position.col.checked_sub(TAB_BAR_COLS)?,
    })
}

const fn pane_position_for_sidebar_drag(
    position: muxr_core::ClientMousePosition,
) -> Option<muxr_core::ClientMousePosition> {
    if position.col >= TAB_BAR_COLS {
        return None;
    }
    Some(muxr_core::ClientMousePosition {
        row: position.row,
        col: 0,
    })
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
            // Resize requests use the pane viewport, because left-side host-terminal columns are reserved for tab UI.
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
    use muxr_core::ClientMouseEventPhase;
    use muxr_core::ClientMousePosition;
    use muxr_core::LayoutSnapshot;
    use muxr_core::PaneId;
    use muxr_core::PaneScrollDirection;
    use muxr_core::PaneSnapshot;
    use muxr_core::ServerError;
    use muxr_core::TabId;
    use muxr_core::TabSnapshot;
    use muxr_transport::ServerListener;

    use super::*;
    use crate::render::SynchronizedOutput;

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
    fn test_server_cmd_uses_supplied_executable_for_internal_server() -> rootcause::Result<()> {
        let session: SessionName = "work".parse()?;
        let cmd = server_cmd(&session, Path::new("/tmp/custom-muxr"));
        let args: Vec<_> = cmd.get_args().collect();

        pretty_assertions::assert_eq!(cmd.get_program(), OsStr::new("/tmp/custom-muxr"));
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
            let key = ClientKey {
                code: ClientKeyCode::Char('E'),
                modifiers: ClientKeyModifiers::SHIFT_ALT,
                raw_bytes: b"\x1bE".to_vec(),
            };
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
        let key = ClientKey {
            code: ClientKeyCode::Char('E'),
            modifiers: ClientKeyModifiers::SHIFT_ALT,
            raw_bytes: b"\x1bE".to_vec(),
        };

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
        let event = ClientMouseEvent {
            button: 0,
            phase: ClientMouseEventPhase::Press,
            position: muxr_core::ClientMousePosition { row: 4, col: 9 },
        };

        assert2::assert!(send_decoded_input(&input_sender, vec![DecodedInput::Mouse(event)]));

        pretty_assertions::assert_eq!(input_receiver.blocking_recv(), Some(ClientInputAction::Mouse(event)),);
    }

    #[test]
    fn test_send_decoded_input_when_mouse_motion_action_queue_is_full_drops_without_blocking() -> rootcause::Result<()>
    {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
        assert2::assert!(input_sender.try_send(ClientInputAction::CopySelection).is_ok());
        let event = ClientMouseEvent {
            button: 32,
            phase: ClientMouseEventPhase::Press,
            position: muxr_core::ClientMousePosition { row: 4, col: 9 },
        };
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
    fn test_send_decoded_input_when_mouse_wheel_action_queue_is_full_waits_for_queue_space() -> rootcause::Result<()> {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
        assert2::assert!(input_sender.try_send(ClientInputAction::CopySelection).is_ok());
        let event = ClientMouseEvent {
            button: 64,
            phase: ClientMouseEventPhase::Press,
            position: muxr_core::ClientMousePosition { row: 4, col: 9 },
        };
        let (result_sender, result_receiver) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            let _ = result_sender.send(send_decoded_input(&input_sender, vec![DecodedInput::Mouse(event)]));
        });

        assert2::assert!(result_receiver.recv_timeout(Duration::from_millis(50)).is_err());
        pretty_assertions::assert_eq!(input_receiver.blocking_recv(), Some(ClientInputAction::CopySelection));
        pretty_assertions::assert_eq!(result_receiver.recv_timeout(Duration::from_secs(1)), Ok(true));
        pretty_assertions::assert_eq!(input_receiver.blocking_recv(), Some(ClientInputAction::Mouse(event)));
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
                    ClientInputAction::Mouse(ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Press,
                        position: muxr_core::ClientMousePosition {
                            row: 0,
                            col: TAB_BAR_COLS.saturating_add(1)
                        }
                    }),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::FocusPaneAt(muxr_core::ClientMousePosition {
                    row: 0,
                    col: 1
                })),
            );
            Ok(())
        })
    }

    #[test]
    fn test_handle_client_input_action_when_tab_sidebar_is_clicked_focuses_tab() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
            let mut renderer = ClientRenderer::with_synchronized_output(
                two_tab_layout()?,
                pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();

            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Press,
                        position: muxr_core::ClientMousePosition { row: 3, col: 1 }
                    }),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::FocusTab(TabId::new(2)?)),
            );
            pretty_assertions::assert_eq!(output.flushes, 0);
            Ok(())
        })
    }

    #[test]
    fn test_handle_client_input_action_when_selection_release_is_on_tab_sidebar_finalizes_selection()
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
                    ClientInputAction::Mouse(ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Press,
                        position: muxr_core::ClientMousePosition {
                            row: 0,
                            col: TAB_BAR_COLS.saturating_add(1)
                        }
                    }),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Release,
                        position: muxr_core::ClientMousePosition { row: 0, col: 1 }
                    }),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::FocusPaneAt(muxr_core::ClientMousePosition {
                    row: 0,
                    col: 1
                })),
            );
            pretty_assertions::assert_eq!(renderer.selected_text(), Some("ab".to_owned()),);
            Ok(())
        })
    }

    #[test]
    fn test_handle_client_input_action_when_selection_drag_moves_into_tab_sidebar_clamps_to_left_edge()
    -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(2);
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
                    ClientInputAction::Mouse(ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Press,
                        position: muxr_core::ClientMousePosition {
                            row: 0,
                            col: TAB_BAR_COLS.saturating_add(1)
                        }
                    }),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent {
                        button: 32,
                        phase: ClientMouseEventPhase::Press,
                        position: muxr_core::ClientMousePosition { row: 0, col: 0 }
                    }),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::FocusPaneAt(muxr_core::ClientMousePosition {
                    row: 0,
                    col: 1
                })),
            );
            assert2::assert!(matches!(
                input_receiver.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ));
            pretty_assertions::assert_eq!(renderer.selected_text(), Some("ab".to_owned()),);
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
        renderer.apply_selection_input(
            &mut output,
            SelectionInput::Start(ClientMousePosition { row: 0, col: 0 }),
        )?;
        let initial = renderer
            .set_selection_edge_drag(ClientMousePosition { row: 2, col: 1 }, None)
            .ok_or_else(|| report!("expected initial muxr edge scroll request"))?;
        let expected = ClientRequest::ScrollPaneLineAt {
            direction: PaneScrollDirection::Down,
            position: ClientMousePosition { row: 0, col: 1 },
        };
        pretty_assertions::assert_eq!(initial.request(), &expected);
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
        renderer.apply_selection_input(
            &mut output,
            SelectionInput::Start(ClientMousePosition { row: 0, col: 0 }),
        )?;
        let request = renderer
            .set_selection_edge_drag(ClientMousePosition { row: 2, col: 1 }, None)
            .ok_or_else(|| report!("expected muxr edge scroll request"))?;

        assert2::assert!(send_edge_scroll_request(&input_sender, &mut renderer, request));
        pretty_assertions::assert_eq!(input_receiver.try_recv(), Ok(ClientRequest::Pong));
        assert2::assert!(send_selection_edge_scroll_request(&input_sender, &mut renderer));

        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientRequest::ScrollPaneLineAt {
                direction: PaneScrollDirection::Down,
                position: ClientMousePosition { row: 0, col: 1 },
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
                    ClientInputAction::Mouse(ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Press,
                        position: muxr_core::ClientMousePosition {
                            row: 0,
                            col: TAB_BAR_COLS.saturating_add(1)
                        }
                    }),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    button: 0,
                    phase: ClientMouseEventPhase::Press,
                    position: muxr_core::ClientMousePosition { row: 0, col: 1 }
                })),
            );
            pretty_assertions::assert_eq!(output.flushes, 0);
            Ok(())
        })
    }

    #[test]
    fn test_handle_client_input_action_when_pane_receives_wheel_forwards_mouse_to_server() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
            let mut renderer = ClientRenderer::with_synchronized_output(
                layout_snapshot()?,
                pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();
            let event = ClientMouseEvent {
                button: 64,
                phase: ClientMouseEventPhase::Press,
                position: muxr_core::ClientMousePosition {
                    row: 0,
                    col: TAB_BAR_COLS.saturating_add(1),
                },
            };

            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(event),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    position: muxr_core::ClientMousePosition { row: 0, col: 1 },
                    ..event
                })),
            );
            pretty_assertions::assert_eq!(output.flushes, 0);
            Ok(())
        })
    }

    #[test]
    fn test_handle_client_input_action_when_pane_wheel_request_queue_is_full_waits_for_queue_space()
    -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
            assert2::assert!(input_sender.try_send(ClientRequest::Pong).is_ok());
            let mut renderer = ClientRenderer::with_synchronized_output(
                layout_snapshot()?,
                pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();
            let event = ClientMouseEvent {
                button: 64,
                phase: ClientMouseEventPhase::Press,
                position: muxr_core::ClientMousePosition {
                    row: 0,
                    col: TAB_BAR_COLS.saturating_add(1),
                },
            };
            let handle = handle_client_input_action(
                ClientInputAction::Mouse(event),
                &input_sender,
                &mut renderer,
                &mut output,
            );
            tokio::pin!(handle);

            tokio::select! {
                result = &mut handle => {
                    return Err(report!("muxr wheel request did not wait for queue space").attach(format!("{result:?}")));
                }
                () = tokio::time::sleep(Duration::from_millis(50)) => {}
            }

            pretty_assertions::assert_eq!(input_receiver.recv().await, Some(ClientRequest::Pong));
            assert2::assert!(handle.await?);
            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    position: muxr_core::ClientMousePosition { row: 0, col: 1 },
                    ..event
                })),
            );
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
                    ClientInputAction::Mouse(ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Press,
                        position: muxr_core::ClientMousePosition {
                            row: 0,
                            col: TAB_BAR_COLS.saturating_add(1)
                        }
                    }),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent {
                        button: 32,
                        phase: ClientMouseEventPhase::Press,
                        position: muxr_core::ClientMousePosition {
                            row: 0,
                            col: TAB_BAR_COLS.saturating_add(3)
                        }
                    }),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Release,
                        position: muxr_core::ClientMousePosition { row: 0, col: 1 }
                    }),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_client_input_action(
                    ClientInputAction::Mouse(ClientMouseEvent {
                        button: 32,
                        phase: ClientMouseEventPhase::Press,
                        position: muxr_core::ClientMousePosition {
                            row: 0,
                            col: TAB_BAR_COLS.saturating_add(3)
                        }
                    }),
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    button: 0,
                    phase: ClientMouseEventPhase::Press,
                    position: muxr_core::ClientMousePosition { row: 0, col: 1 }
                })),
            );
            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    button: 32,
                    phase: ClientMouseEventPhase::Press,
                    position: muxr_core::ClientMousePosition { row: 0, col: 1 }
                })),
            );
            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    button: 0,
                    phase: ClientMouseEventPhase::Release,
                    position: muxr_core::ClientMousePosition { row: 0, col: 0 }
                })),
            );
            assert2::assert!(matches!(
                input_receiver.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ));
            Ok(())
        })
    }

    #[test]
    fn test_pane_size_for_terminal_when_tab_bar_has_room_reserves_sidebar_columns() -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(
            pane_size_for_terminal(&TerminalSize::new(80, 24)?)?,
            TerminalSize::new(80_u16.saturating_sub(TAB_BAR_COLS), 24)?,
        );
        pretty_assertions::assert_eq!(
            pane_size_for_terminal(&TerminalSize::new(80, 1)?)?,
            TerminalSize::new(80_u16.saturating_sub(TAB_BAR_COLS), 1)?,
        );
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

    async fn recv_client_request(
        input_receiver: &mut tokio::sync::mpsc::Receiver<ClientRequest>,
    ) -> rootcause::Result<Option<ClientRequest>> {
        Ok(tokio::time::timeout(Duration::from_secs(1), input_receiver.recv())
            .await
            .context("timed out waiting for muxr client request")?)
    }

    fn layout_snapshot() -> rootcause::Result<LayoutSnapshot> {
        let active_tab = TabId::new(1)?;
        let active_pane = PaneId::new(1)?;
        let pane = PaneSnapshot {
            agent_state: muxr_core::PaneAgentState::NoAgent,
            cwd: "/tmp".to_owned(),
            cmd_label: None,
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

    fn two_tab_layout() -> rootcause::Result<LayoutSnapshot> {
        LayoutSnapshot::new(
            TabId::new(1)?,
            vec![
                TabSnapshot::new(
                    TabId::new(1)?,
                    "default",
                    PaneId::new(1)?,
                    vec![PaneSnapshot {
                        agent_state: muxr_core::PaneAgentState::NoAgent,
                        cwd: "/tmp/tab-1".to_owned(),
                        cmd_label: None,
                        id: PaneId::new(1)?,
                        title: "shell".to_owned(),
                    }],
                )?,
                TabSnapshot::new(
                    TabId::new(2)?,
                    "tab 2",
                    PaneId::new(2)?,
                    vec![PaneSnapshot {
                        agent_state: muxr_core::PaneAgentState::NoAgent,
                        cwd: "/tmp/tab-2".to_owned(),
                        cmd_label: None,
                        id: PaneId::new(2)?,
                        title: "shell".to_owned(),
                    }],
                )?,
            ],
        )
    }

    fn mouse_tracking_pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![muxr_core::PaneRegionSnapshot::new(
            muxr_core::PaneId::new(1)?,
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
                muxr_core::PaneId::new(1)?,
                0,
                0,
                2,
                1,
                muxr_core::PaneMouseMode::ButtonMotion,
                0,
            )?,
            muxr_core::PaneRegionSnapshot::new(
                muxr_core::PaneId::new(2)?,
                2,
                0,
                2,
                1,
                muxr_core::PaneMouseMode::None,
                0,
            )?,
        ])
    }

    fn render_baseline() -> rootcause::Result<muxr_core::RenderBaseline> {
        muxr_core::RenderBaseline::new(
            1,
            TerminalSize::new(2, 1)?,
            muxr_core::RenderCursor {
                row: 0,
                col: 1,
                visible: true,
            },
            vec![muxr_core::RenderRowSpan::new(
                0,
                0,
                vec![render_cell("a"), render_cell("b")],
            )?],
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
