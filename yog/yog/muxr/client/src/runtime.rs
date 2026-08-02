use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::Duration;

use muxr_config::MuxrConfig;
use muxr_core::ClientMouseEvent;
use muxr_core::ClientRequest;
use muxr_core::ServerEvent;
use muxr_core::SessionName;
use muxr_core::TerminalSize;
use muxr_transport::ClientRequestWriter;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::copy_selection::SelectionEdgeScrollRequest;
use crate::input::DecodedInput;
use crate::input::InputDecoder;
use crate::input::InputIdleTimeout;
use crate::renderer::ClientRenderOutcome;
use crate::renderer::ClientRenderer;
use crate::session::attach::AttachedSession;
use crate::stdout_worker::StdoutSender;
use crate::stdout_worker::StdoutWorker;
use crate::terminal::TerminalGuard;

const RESIZE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const AMBIGUOUS_INPUT_TIMEOUT: Duration = Duration::from_millis(50);
const SELECTION_EDGE_SCROLL_INTERVAL: Duration = Duration::from_millis(50);
const STDIN_BUFFER_SIZE: usize = 8192;
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
    CopySelectionInline,
    Mouse(ClientMouseEvent),
}

#[derive(Debug)]
enum ClientInputCmd {
    Action(ClientInputAction),
    Barrier(std::sync::mpsc::SyncSender<()>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputCmdReceiverState {
    Open,
    Closed,
}

impl InputCmdReceiverState {
    const fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalActionCompletion {
    Wait,
    #[cfg(test)]
    Skip,
}

impl LocalActionCompletion {
    const fn waits(self) -> bool {
        match self {
            Self::Wait => true,
            #[cfg(test)]
            Self::Skip => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientInputSend {
    Accepted,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InteractiveFlow {
    Continue,
    Stop,
}

/// Start or attach to a muxr session and run an interactive client.
///
/// # Errors
/// - The session paths cannot be resolved.
/// - The server cannot be started or attached.
/// - The current terminal size cannot be read.
/// - Terminal input/output or protocol IO fails.
pub fn start(session: &SessionName, server_executable: &Path, external_layout: Option<&Path>) -> rootcause::Result<()> {
    tokio::runtime::Runtime::new()
        .context("failed to build muxr tokio runtime")?
        .block_on(async {
            let muxr_config = MuxrConfig::default();
            let terminal_size = crate::terminal::current_terminal_size()?;
            let pane_size = crate::terminal::pane_size_for_terminal(muxr_config.tab_bar.width, &terminal_size)?;
            let attached_session =
                crate::session::attach::open_session(session, pane_size.clone(), server_executable, external_layout)
                    .await?;
            self::run_interactive(&muxr_config, attached_session, pane_size).await
        })
}

async fn run_interactive(
    muxr_config: &MuxrConfig,
    mut attached_session: AttachedSession,
    initial_size: TerminalSize,
) -> rootcause::Result<()> {
    let _terminal_guard = TerminalGuard::enable_if_terminal()?;
    let (control_sender, control_receiver) = tokio::sync::mpsc::channel(CONTROL_REQUEST_CHANNEL_LIMIT);
    let (input_cmd_sender, mut input_cmd_receiver) = tokio::sync::mpsc::channel(INPUT_REQUEST_CHANNEL_LIMIT);
    let (input_request_sender, input_receiver) = tokio::sync::mpsc::channel(INPUT_REQUEST_CHANNEL_LIMIT);
    let stdin_handle = self::spawn_stdin_forwarder(input_cmd_sender, input_request_sender.clone());
    let resize_handle = self::spawn_resize_forwarder(control_sender.clone(), muxr_config.tab_bar.width, initial_size);
    let writer = attached_session.writer;
    let writer_handle =
        tokio::spawn(async move { self::forward_client_requests(writer, control_receiver, input_receiver).await });
    let (stdout_sender, _stdout_worker, mut stdout_failure_receiver) = StdoutWorker::spawn();
    let mut renderer = ClientRenderer::new(muxr_config, attached_session.layout, attached_session.pane_regions);
    let mut transaction = Vec::new();
    renderer.sync_mouse_capture(&mut transaction)?;
    stdout_sender.send_mandatory(transaction)?;
    let edge_scroll_tick_start = tokio::time::Instant::now()
        .checked_add(SELECTION_EDGE_SCROLL_INTERVAL)
        .ok_or_else(|| report!("muxr selection edge scroll interval overflowed"))?;
    let mut edge_scroll_tick = tokio::time::interval_at(edge_scroll_tick_start, SELECTION_EDGE_SCROLL_INTERVAL);
    edge_scroll_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut input_cmd_receiver_state = InputCmdReceiverState::Open;

    loop {
        tokio::select! {
            event = attached_session.reader.recv_event() => {
                let Some(event) = event? else {
                    break;
                };
                if self::handle_server_event(
                    event,
                    &control_sender,
                    &mut renderer,
                    &stdout_sender,
                ).await?
                    == InteractiveFlow::Stop
                {
                    break;
                }
            },
            cmd = input_cmd_receiver.recv(), if input_cmd_receiver_state.is_open() => {
                let Some(cmd) = cmd else {
                    input_cmd_receiver_state = InputCmdReceiverState::Closed;
                    continue;
                };
                let action = match cmd {
                    ClientInputCmd::Action(action) => action,
                    ClientInputCmd::Barrier(completed) => {
                        let _sent = completed.send(());
                        continue;
                    }
                };
                let mut transaction = Vec::new();
                if self::handle_client_input_action(action, muxr_config, &input_request_sender, &mut renderer, &mut transaction).await? == ClientInputSend::Closed {
                    break;
                }
                stdout_sender.send_mandatory(transaction)?;
            },
            _ = edge_scroll_tick.tick(), if renderer.selection_edge_drag() == crate::renderer::SelectionEdgeDrag::Active => {
                if self::send_selection_edge_scroll_request(&input_request_sender, &mut renderer) == ClientInputSend::Closed {
                    break;
                }
            },
            stdout_failure = &mut stdout_failure_receiver => {
                let error = stdout_failure.unwrap_or_else(|_| "stdout worker stopped unexpectedly".to_owned());
                return Err(report!("muxr client stdout worker failed").attach(error));
            },
            else => {
                if input_cmd_receiver_state == InputCmdReceiverState::Closed {
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

async fn handle_server_event(
    event: ServerEvent,
    control_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    renderer: &mut ClientRenderer,
    stdout_sender: &StdoutSender,
) -> rootcause::Result<InteractiveFlow> {
    match event {
        ServerEvent::Deleted | ServerEvent::Detached => Ok(InteractiveFlow::Stop),
        ServerEvent::Error(error) => Err(report!("muxr server returned error")
            .attach(format!("code={}", error.code()))
            .attach(format!("msg={}", error.msg()))),
        ServerEvent::Ping => Ok(if control_sender.send(ClientRequest::Pong).await.is_ok() {
            InteractiveFlow::Continue
        } else {
            InteractiveFlow::Stop
        }),
        ServerEvent::Layout(next_layout) => {
            renderer.apply_layout(next_layout);
            Ok(InteractiveFlow::Continue)
        }
        ServerEvent::SidebarLayout(next_layout) => {
            let mut transaction = Vec::new();
            renderer.apply_sidebar_layout(&mut transaction, next_layout)?;
            stdout_sender.send_mandatory(transaction)?;
            Ok(InteractiveFlow::Continue)
        }
        ServerEvent::PaneRegions(next_regions) => {
            let mut transaction = Vec::new();
            renderer.apply_pane_regions(&mut transaction, next_regions)?;
            stdout_sender.send_mandatory(transaction)?;
            Ok(InteractiveFlow::Continue)
        }
        ServerEvent::Render(update) => self::handle_render_event(update, control_sender, renderer, stdout_sender).await,
        ServerEvent::ScrollPaneLineResult {
            position,
            direction,
            movement,
        } => {
            renderer.apply_scroll_pane_line_result(position, direction, movement);
            Ok(InteractiveFlow::Continue)
        }
        ServerEvent::Attached(_) | ServerEvent::Pong => Ok(InteractiveFlow::Continue),
    }
}

async fn handle_render_event(
    update: muxr_core::RenderUpdate,
    control_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    renderer: &mut ClientRenderer,
    stdout_sender: &StdoutSender,
) -> rootcause::Result<InteractiveFlow> {
    let mut transaction = Vec::new();
    match renderer.apply_render(&mut transaction, update)? {
        ClientRenderOutcome::Drawn => {
            stdout_sender.replace_render(transaction, || renderer.full_redraw_transaction())?;
            Ok(InteractiveFlow::Continue)
        }
        ClientRenderOutcome::NeedsResync => Ok(if control_sender.send(ClientRequest::RenderResync).await.is_ok() {
            InteractiveFlow::Continue
        } else {
            InteractiveFlow::Stop
        }),
    }
}

async fn handle_client_input_action(
    action: ClientInputAction,
    muxr_config: &MuxrConfig,
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    renderer: &mut ClientRenderer,
    stdout: &mut impl Write,
) -> rootcause::Result<ClientInputSend> {
    match action {
        ClientInputAction::CopySelection => {
            renderer.copy_selection()?;
            Ok(ClientInputSend::Accepted)
        }
        ClientInputAction::CopySelectionInline => {
            renderer.copy_selection_inline()?;
            Ok(ClientInputSend::Accepted)
        }
        ClientInputAction::Mouse(event) => {
            crate::pane::mouse::handle_mouse_input_action(muxr_config, event, input_sender, renderer, stdout).await
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DroppableSendOutcome {
    Closed,
    Dropped,
    Sent,
}

pub fn send_droppable_request(
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

pub fn send_edge_scroll_request(
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    renderer: &mut ClientRenderer,
    request: SelectionEdgeScrollRequest,
) -> ClientInputSend {
    let (pending, request) = request.into_parts();
    match self::send_droppable_request(input_sender, request) {
        DroppableSendOutcome::Sent => {
            // One queued edge-scroll request must be paired with one moved viewport and its render before another
            // request is queued; otherwise coalesced renders can skip selected content rows.
            renderer.mark_selection_edge_scroll_sent(pending);
            ClientInputSend::Accepted
        }
        DroppableSendOutcome::Dropped => ClientInputSend::Accepted,
        DroppableSendOutcome::Closed => ClientInputSend::Closed,
    }
}

fn send_selection_edge_scroll_request(
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    renderer: &mut ClientRenderer,
) -> ClientInputSend {
    let Some(request) = renderer.selection_edge_scroll_request() else {
        return ClientInputSend::Accepted;
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

fn spawn_stdin_forwarder(
    cmd_sender: tokio::sync::mpsc::Sender<ClientInputCmd>,
    request_sender: tokio::sync::mpsc::Sender<ClientRequest>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (read_sender, read_receiver) = std::sync::mpsc::channel();
        drop(self::spawn_stdin_reader(read_sender));
        let mut decoder = InputDecoder::default();

        loop {
            // Ambiguous escape prefixes need an idle timeout for bare Esc. Bracketed paste waits for its terminator so
            // slow multi-chunk paste cannot leak raw paste markers into the PTY.
            let read = if decoder.idle_timeout() == InputIdleTimeout::Needed {
                match read_receiver.recv_timeout(AMBIGUOUS_INPUT_TIMEOUT) {
                    Ok(read) => read,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if self::send_decoded_input_with_ordering(
                            &cmd_sender,
                            &request_sender,
                            decoder.finalize(),
                            LocalActionCompletion::Wait,
                        ) == ClientInputSend::Closed
                        {
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
                    if self::send_decoded_input_with_ordering(
                        &cmd_sender,
                        &request_sender,
                        decoder.decode(&bytes),
                        LocalActionCompletion::Wait,
                    ) == ClientInputSend::Closed
                    {
                        break;
                    }
                }
                StdinRead::Eof => {
                    if self::send_decoded_input_with_ordering(
                        &cmd_sender,
                        &request_sender,
                        decoder.finalize(),
                        LocalActionCompletion::Wait,
                    ) == ClientInputSend::Closed
                    {
                        break;
                    }
                    // EOF detach follows any queued stdin bytes so piped cmds like `exit\n` reach the shell first.
                    drop(request_sender.blocking_send(ClientRequest::Detach));
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

#[cfg(test)]
fn send_decoded_input(
    cmd_sender: &tokio::sync::mpsc::Sender<ClientInputCmd>,
    request_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    decoded: Vec<DecodedInput>,
) -> ClientInputSend {
    self::send_decoded_input_with_ordering(cmd_sender, request_sender, decoded, LocalActionCompletion::Skip)
}

fn send_decoded_input_with_ordering(
    cmd_sender: &tokio::sync::mpsc::Sender<ClientInputCmd>,
    request_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    decoded: Vec<DecodedInput>,
    local_action_completion: LocalActionCompletion,
) -> ClientInputSend {
    for decoded in decoded {
        let action = match decoded {
            DecodedInput::CopySelection => ClientInputAction::CopySelection,
            DecodedInput::CopySelectionInline => ClientInputAction::CopySelectionInline,
            // These requests do not need local renderer state. Send them directly to the sole request writer so
            // terminal encoding and selection rebuilding cannot delay PTY input while another pane is repainting.
            DecodedInput::Input(bytes) => {
                if request_sender.blocking_send(ClientRequest::Input(bytes)).is_err() {
                    return ClientInputSend::Closed;
                }
                continue;
            }
            DecodedInput::Key(key) => {
                if request_sender.blocking_send(ClientRequest::Key(key)).is_err() {
                    return ClientInputSend::Closed;
                }
                continue;
            }
            DecodedInput::Mouse(event)
                if crate::pane::mouse::MouseEventDrop::from(event) == crate::pane::mouse::MouseEventDrop::Droppable =>
            {
                if self::send_droppable_input_action(
                    cmd_sender,
                    ClientInputAction::Mouse(event),
                    local_action_completion,
                ) == ClientInputSend::Closed
                {
                    return ClientInputSend::Closed;
                }
                continue;
            }
            DecodedInput::Mouse(event) => ClientInputAction::Mouse(event),
            DecodedInput::Paste(bytes) => {
                if request_sender.blocking_send(ClientRequest::Paste(bytes)).is_err() {
                    return ClientInputSend::Closed;
                }
                continue;
            }
        };
        if self::send_input_action(cmd_sender, action, local_action_completion) == ClientInputSend::Closed {
            return ClientInputSend::Closed;
        }
    }

    ClientInputSend::Accepted
}

fn send_droppable_input_action(
    cmd_sender: &tokio::sync::mpsc::Sender<ClientInputCmd>,
    action: ClientInputAction,
    local_action_completion: LocalActionCompletion,
) -> ClientInputSend {
    match cmd_sender.try_send(ClientInputCmd::Action(action)) {
        Ok(()) if local_action_completion.waits() => self::send_input_action_barrier(cmd_sender),
        Ok(()) => ClientInputSend::Accepted,
        Err(tokio::sync::mpsc::error::TrySendError::Full(action)) => {
            drop(action);
            ClientInputSend::Accepted
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(action)) => {
            drop(action);
            ClientInputSend::Closed
        }
    }
}

fn send_input_action(
    cmd_sender: &tokio::sync::mpsc::Sender<ClientInputCmd>,
    action: ClientInputAction,
    local_action_completion: LocalActionCompletion,
) -> ClientInputSend {
    if cmd_sender.blocking_send(ClientInputCmd::Action(action)).is_err() {
        return ClientInputSend::Closed;
    }
    if local_action_completion.waits() {
        self::send_input_action_barrier(cmd_sender)
    } else {
        ClientInputSend::Accepted
    }
}

fn send_input_action_barrier(cmd_sender: &tokio::sync::mpsc::Sender<ClientInputCmd>) -> ClientInputSend {
    let (completed_sender, completed_receiver) = std::sync::mpsc::sync_channel(0);
    if cmd_sender
        .blocking_send(ClientInputCmd::Barrier(completed_sender))
        .is_err()
    {
        return ClientInputSend::Closed;
    }
    if completed_receiver.recv().is_err() {
        return ClientInputSend::Closed;
    }
    ClientInputSend::Accepted
}

fn spawn_resize_forwarder(
    sender: tokio::sync::mpsc::Sender<ClientRequest>,
    tab_bar_width: u16,
    initial_size: TerminalSize,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut last_size = initial_size;

        loop {
            if sender.is_closed() {
                break;
            }

            thread::sleep(RESIZE_POLL_INTERVAL);
            let Ok(next_terminal_size) = crate::terminal::current_terminal_size() else {
                break;
            };
            // Resize requests use the pane viewport, because left-side host-terminal columns are reserved for tab UI.
            let Ok(next_size) = crate::terminal::pane_size_for_terminal(tab_bar_width, &next_terminal_size) else {
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::path::Path;

    use muxr_core::ClientKey;
    use muxr_core::ClientKeyCode;
    use muxr_core::ClientKeyModifiers;
    use muxr_core::ClientMouseEventPhase;
    use muxr_core::ClientMousePosition;
    use muxr_core::LayoutSnapshot;
    use muxr_core::PaneId;
    use muxr_core::PaneRegionsSnapshot;
    use muxr_core::PaneScrollDirection;
    use muxr_core::PaneSnapshot;
    use muxr_core::SessionPaths;
    use muxr_core::TabId;
    use muxr_core::TabSnapshot;
    use muxr_transport::ClientConnection;
    use muxr_transport::ServerListener;
    use test_that::prelude::*;

    use super::*;
    use crate::copy_selection::SelectionInput;
    use crate::copy_selection::test_helpers as copy_selection_test_helpers;
    use crate::terminal::SynchronizedOutput;

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
            assert_that!(input_sender.try_send(ClientRequest::Input(vec![b'a'])), ok(eq(())));
            assert_that!(input_sender.try_send(ClientRequest::Input(vec![b'b'])), err(anything()));
            assert_that!(control_sender.try_send(ClientRequest::Pong), ok(eq(())));

            let writer_handle = tokio::spawn(self::forward_client_requests(writer, control_receiver, input_receiver));
            let first_request = server_handle
                .await
                .map_err(|error| report!("muxr forward test socket task panicked").attach(format!("{error}")))??;

            assert_that!(first_request, eq(ClientRequest::Pong));
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
            assert_that!(input_sender.try_send(ClientRequest::Input(b"a".to_vec())), ok(eq(())));
            assert_that!(input_sender.try_send(ClientRequest::Key(key.clone())), ok(eq(())));
            assert_that!(input_sender.try_send(ClientRequest::Input(b"b".to_vec())), ok(eq(())));
            drop(control_sender);
            drop(input_sender);

            let writer_handle = tokio::spawn(self::forward_client_requests(writer, control_receiver, input_receiver));
            let requests = server_handle.await.map_err(|error| {
                report!("muxr forward order test socket task panicked").attach(format!("{error}"))
            })??;

            assert_that!(
                requests,
                eq(vec![
                    ClientRequest::Input(b"a".to_vec()),
                    ClientRequest::Key(key),
                    ClientRequest::Input(b"b".to_vec()),
                ])
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
            assert_that!(
                input_sender.try_send(ClientRequest::Input(b"exit\n".to_vec())),
                ok(eq(()))
            );
            assert_that!(input_sender.try_send(ClientRequest::Detach), ok(eq(())));
            drop(control_sender);
            drop(input_sender);

            let writer_handle = tokio::spawn(self::forward_client_requests(writer, control_receiver, input_receiver));
            let requests = server_handle
                .await
                .map_err(|error| report!("muxr forward EOF test socket task panicked").attach(format!("{error}")))??;

            assert_that!(
                requests,
                eq(vec![ClientRequest::Input(b"exit\n".to_vec()), ClientRequest::Detach])
            );
            writer_handle
                .await
                .map_err(|error| report!("muxr forward EOF test writer task panicked").attach(format!("{error}")))??;
            Ok(())
        })
    }

    #[test]
    fn test_send_decoded_input_when_key_arrives_bypasses_the_renderer_action_queue_in_order() {
        let (cmd_sender, mut cmd_receiver) = tokio::sync::mpsc::channel(1);
        let (request_sender, mut request_receiver) = tokio::sync::mpsc::channel(3);
        let key = ClientKey {
            code: ClientKeyCode::Char('E'),
            modifiers: ClientKeyModifiers::SHIFT_ALT,
            raw_bytes: b"\x1bE".to_vec(),
        };

        assert_that!(
            send_decoded_input(
                &cmd_sender,
                &request_sender,
                vec![
                    DecodedInput::Input(b"a".to_vec()),
                    DecodedInput::Key(key.clone()),
                    DecodedInput::Input(b"b".to_vec()),
                ],
            ),
            eq(ClientInputSend::Accepted)
        );

        assert_that!(
            request_receiver.blocking_recv(),
            eq(Some(ClientRequest::Input(b"a".to_vec())))
        );
        assert_that!(request_receiver.blocking_recv(), eq(Some(ClientRequest::Key(key))));
        assert_that!(
            request_receiver.blocking_recv(),
            eq(Some(ClientRequest::Input(b"b".to_vec())))
        );
        assert_that!(cmd_receiver.try_recv().is_err(), eq(true));
    }

    #[test]
    fn test_send_decoded_input_when_scrollback_editor_shortcut_arrives_sends_key_request() {
        let (cmd_sender, _cmd_receiver) = tokio::sync::mpsc::channel(1);
        let (request_sender, mut request_receiver) = tokio::sync::mpsc::channel(1);
        let key = ClientKey {
            code: ClientKeyCode::Char('S'),
            modifiers: ClientKeyModifiers::SHIFT_ALT,
            raw_bytes: b"\x1bS".to_vec(),
        };

        assert_that!(
            send_decoded_input(&cmd_sender, &request_sender, vec![DecodedInput::Key(key.clone())]),
            eq(ClientInputSend::Accepted)
        );

        assert_that!(request_receiver.blocking_recv(), eq(Some(ClientRequest::Key(key))));
    }

    #[test]
    fn test_send_decoded_input_when_paste_arrives_uses_input_queue() {
        let (cmd_sender, _cmd_receiver) = tokio::sync::mpsc::channel(1);
        let (request_sender, mut request_receiver) = tokio::sync::mpsc::channel(1);

        assert_that!(
            send_decoded_input(
                &cmd_sender,
                &request_sender,
                vec![DecodedInput::Paste(b"one\ntwo\n".to_vec())],
            ),
            eq(ClientInputSend::Accepted)
        );

        assert_that!(
            request_receiver.blocking_recv(),
            eq(Some(ClientRequest::Paste(b"one\ntwo\n".to_vec())))
        );
    }

    #[test]
    fn test_send_decoded_input_when_mouse_arrives_emits_local_mouse_action() {
        let (cmd_sender, mut cmd_receiver) = tokio::sync::mpsc::channel(1);
        let (request_sender, _request_receiver) = tokio::sync::mpsc::channel(1);
        let event = ClientMouseEvent {
            button: 0,
            phase: ClientMouseEventPhase::Press,
            position: muxr_core::ClientMousePosition { row: 4, col: 9 },
        };

        assert_that!(
            send_decoded_input(&cmd_sender, &request_sender, vec![DecodedInput::Mouse(event)]),
            eq(ClientInputSend::Accepted)
        );

        assert_that!(
            matches!(
                cmd_receiver.blocking_recv(),
                Some(ClientInputCmd::Action(ClientInputAction::Mouse(actual))) if actual == event
            ),
            eq(true)
        );
    }

    #[test]
    fn test_send_decoded_input_when_mouse_motion_action_queue_is_full_drops_without_blocking() -> rootcause::Result<()>
    {
        let (cmd_sender, mut cmd_receiver) = tokio::sync::mpsc::channel(1);
        let (request_sender, _request_receiver) = tokio::sync::mpsc::channel(1);
        assert_that!(
            cmd_sender.try_send(ClientInputCmd::Action(ClientInputAction::CopySelection)),
            ok(eq(()))
        );
        let event = ClientMouseEvent {
            button: 32,
            phase: ClientMouseEventPhase::Press,
            position: muxr_core::ClientMousePosition { row: 4, col: 9 },
        };
        let (result_sender, result_receiver) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            let _ = result_sender.send(send_decoded_input(
                &cmd_sender,
                &request_sender,
                vec![DecodedInput::Mouse(event)],
            ));
        });
        let result = match result_receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(result) => result,
            Err(error) => {
                drop(cmd_receiver);
                handle
                    .join()
                    .map_err(|error| report!("muxr mouse input test thread panicked").attach(format!("{error:?}")))?;
                return Err(report!("muxr mouse motion blocked on full input-action queue").attach(format!("{error}")));
            }
        };

        assert_that!(result, eq(ClientInputSend::Accepted));
        assert_that!(
            matches!(
                cmd_receiver.try_recv(),
                Ok(ClientInputCmd::Action(ClientInputAction::CopySelection))
            ),
            eq(true)
        );
        assert_that!(cmd_receiver.try_recv(), err(anything()));
        handle
            .join()
            .map_err(|error| report!("muxr mouse input test thread panicked").attach(format!("{error:?}")))?;
        Ok(())
    }

    #[test]
    fn test_send_decoded_input_when_mouse_wheel_action_queue_is_full_waits_for_queue_space() -> rootcause::Result<()> {
        let (cmd_sender, mut cmd_receiver) = tokio::sync::mpsc::channel(1);
        let (request_sender, _request_receiver) = tokio::sync::mpsc::channel(1);
        assert_that!(
            cmd_sender.try_send(ClientInputCmd::Action(ClientInputAction::CopySelection)),
            ok(eq(()))
        );
        let event = ClientMouseEvent {
            button: 64,
            phase: ClientMouseEventPhase::Press,
            position: muxr_core::ClientMousePosition { row: 4, col: 9 },
        };
        let (result_sender, result_receiver) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            let _ = result_sender.send(send_decoded_input(
                &cmd_sender,
                &request_sender,
                vec![DecodedInput::Mouse(event)],
            ));
        });

        assert_that!(result_receiver.recv_timeout(Duration::from_millis(50)), err(anything()));
        assert_that!(
            matches!(
                cmd_receiver.blocking_recv(),
                Some(ClientInputCmd::Action(ClientInputAction::CopySelection))
            ),
            eq(true)
        );
        assert_that!(
            result_receiver.recv_timeout(Duration::from_secs(1)),
            eq(Ok(ClientInputSend::Accepted))
        );
        assert_that!(
            matches!(
                cmd_receiver.blocking_recv(),
                Some(ClientInputCmd::Action(ClientInputAction::Mouse(actual))) if actual == event
            ),
            eq(true)
        );
        handle
            .join()
            .map_err(|error| report!("muxr mouse input test thread panicked").attach(format!("{error:?}")))?;
        Ok(())
    }

    #[test]
    fn test_send_decoded_input_when_copy_selection_arrives_emits_local_action() {
        let (cmd_sender, mut cmd_receiver) = tokio::sync::mpsc::channel(1);
        let (request_sender, _request_receiver) = tokio::sync::mpsc::channel(1);

        assert_that!(
            send_decoded_input(&cmd_sender, &request_sender, vec![DecodedInput::CopySelection]),
            eq(ClientInputSend::Accepted)
        );

        assert_that!(
            matches!(
                cmd_receiver.blocking_recv(),
                Some(ClientInputCmd::Action(ClientInputAction::CopySelection))
            ),
            eq(true)
        );
    }

    #[test]
    fn test_send_decoded_input_when_inline_copy_selection_arrives_emits_local_action() {
        let (cmd_sender, mut cmd_receiver) = tokio::sync::mpsc::channel(1);
        let (request_sender, _request_receiver) = tokio::sync::mpsc::channel(1);

        assert_that!(
            send_decoded_input(&cmd_sender, &request_sender, vec![DecodedInput::CopySelectionInline]),
            eq(ClientInputSend::Accepted)
        );

        assert_that!(
            matches!(
                cmd_receiver.blocking_recv(),
                Some(ClientInputCmd::Action(ClientInputAction::CopySelectionInline))
            ),
            eq(true)
        );
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
        assert_that!(
            copy_selection_test_helpers::edge_scroll_request(&initial),
            eq(&expected)
        );
        assert_that!(
            send_edge_scroll_request(&input_sender, &mut renderer, initial),
            eq(ClientInputSend::Accepted)
        );
        assert_that!(input_receiver.blocking_recv(), eq(Some(expected.clone())));
        assert_that!(
            send_selection_edge_scroll_request(&input_sender, &mut renderer),
            eq(ClientInputSend::Accepted)
        );
        assert_that!(
            input_receiver.try_recv(),
            err(matches_pattern!(tokio::sync::mpsc::error::TryRecvError::Empty))
        );

        renderer.apply_pane_regions(&mut output, pane_regions_snapshot_with_visible_top_row(1)?)?;
        renderer.apply_render(&mut output, muxr_core::RenderUpdate::Baseline(render_baseline()?))?;
        assert_that!(
            send_selection_edge_scroll_request(&input_sender, &mut renderer),
            eq(ClientInputSend::Accepted)
        );

        assert_that!(input_receiver.blocking_recv(), eq(Some(expected)));
        Ok(())
    }

    #[test]
    fn test_send_edge_scroll_request_when_queue_is_full_does_not_mark_scroll_pending() -> rootcause::Result<()> {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
        assert_that!(input_sender.try_send(ClientRequest::Pong), ok(eq(())));
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

        assert_that!(
            send_edge_scroll_request(&input_sender, &mut renderer, request),
            eq(ClientInputSend::Accepted)
        );
        assert_that!(input_receiver.try_recv(), eq(Ok(ClientRequest::Pong)));
        assert_that!(
            send_selection_edge_scroll_request(&input_sender, &mut renderer),
            eq(ClientInputSend::Accepted)
        );

        assert_that!(
            input_receiver.blocking_recv(),
            eq(Some(ClientRequest::ScrollPaneLineAt {
                direction: PaneScrollDirection::Down,
                position: ClientMousePosition { row: 0, col: 1 },
            }))
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

    fn layout_snapshot() -> rootcause::Result<LayoutSnapshot> {
        let active_tab = TabId::new(1)?;
        let active_pane = PaneId::new(1)?;
        let pane = PaneSnapshot {
            tracked_process_state: muxr_core::TrackedProcessState::None,
            cwd: "/tmp".to_owned(),
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
