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
use crate::renderer::ClientRenderOutcome;
use crate::renderer::ClientRenderer;
use crate::session::attach::AttachedSession;
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
    ServerRequest(ClientRequest),
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
    let (input_action_sender, mut input_action_receiver) = tokio::sync::mpsc::channel(INPUT_REQUEST_CHANNEL_LIMIT);
    let (input_request_sender, input_receiver) = tokio::sync::mpsc::channel(INPUT_REQUEST_CHANNEL_LIMIT);
    let stdin_handle = self::spawn_stdin_forwarder(input_action_sender);
    let resize_handle = self::spawn_resize_forwarder(control_sender.clone(), muxr_config.tab_bar.width, initial_size);
    let writer = attached_session.writer;
    let writer_handle =
        tokio::spawn(async move { self::forward_client_requests(writer, control_receiver, input_receiver).await });
    let mut stdout = std::io::stdout();
    let mut renderer = ClientRenderer::new(muxr_config, attached_session.layout, attached_session.pane_regions);
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
                    ServerEvent::ScrollPaneLineResult {
                        position,
                        direction,
                        movement,
                    } => renderer.apply_scroll_pane_line_result(position, direction, movement),
                    ServerEvent::Attached(_) | ServerEvent::Pong => {}
                }
            },
            action = input_action_receiver.recv(), if !input_actions_closed => {
                let Some(action) = action else {
                    input_actions_closed = true;
                    continue;
                };
                if !self::handle_client_input_action(action, muxr_config, &input_request_sender, &mut renderer, &mut stdout).await? {
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
    muxr_config: &MuxrConfig,
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    renderer: &mut ClientRenderer,
    stdout: &mut impl Write,
) -> rootcause::Result<bool> {
    match action {
        ClientInputAction::CopySelection => {
            renderer.copy_selection()?;
            Ok(true)
        }
        ClientInputAction::CopySelectionInline => {
            renderer.copy_selection_inline()?;
            Ok(true)
        }
        ClientInputAction::Mouse(event) => {
            crate::pane::mouse::handle_mouse_input_action(muxr_config, event, input_sender, renderer, stdout).await
        }
        ClientInputAction::ServerRequest(request) => {
            if input_sender.send(request).await.is_err() {
                return Ok(false);
            }
            Ok(true)
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
            DecodedInput::CopySelectionInline => ClientInputAction::CopySelectionInline,
            DecodedInput::Input(bytes) => ClientInputAction::ServerRequest(ClientRequest::Input(bytes)),
            DecodedInput::Key(key) => {
                // Key events come from stdin; keep them on the input queue so raw-byte fallback cannot overtake earlier
                // PTY bytes.
                ClientInputAction::ServerRequest(ClientRequest::Key(key))
            }
            DecodedInput::Mouse(event) if crate::pane::mouse::mouse_event_can_be_dropped(event) => {
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
    fn test_send_decoded_input_when_scrollback_editor_shortcut_arrives_sends_key_request() {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
        let key = ClientKey {
            code: ClientKeyCode::Char('S'),
            modifiers: ClientKeyModifiers::SHIFT_ALT,
            raw_bytes: b"\x1bS".to_vec(),
        };

        assert2::assert!(send_decoded_input(&input_sender, vec![DecodedInput::Key(key.clone())]));

        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientInputAction::ServerRequest(ClientRequest::Key(key))),
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
    fn test_send_decoded_input_when_inline_copy_selection_arrives_emits_local_action() {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);

        assert2::assert!(send_decoded_input(
            &input_sender,
            vec![DecodedInput::CopySelectionInline]
        ));

        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientInputAction::CopySelectionInline)
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
        pretty_assertions::assert_eq!(copy_selection_test_helpers::edge_scroll_request(&initial), &expected);
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
