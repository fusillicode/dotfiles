use std::fs;
use std::future::Future;
use std::io::IsTerminal;
use std::io::Read;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use muxr_core::ClientHello;
use muxr_core::ClientRequest;
use muxr_core::INTERNAL_SERVER_ARG;
use muxr_core::LayoutSnapshot;
use muxr_core::PROTOCOL_VERSION;
use muxr_core::RenderUpdate;
use muxr_core::ServerEvent;
use muxr_core::ServerPid;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use muxr_core::TerminalSize;
use muxr_transport::ClientConnection;
use muxr_transport::ClientEventReader;
use muxr_transport::ClientRequestWriter;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::input::DecodedInput;
use crate::input::InputDecoder;
use crate::render::ApplyOutcome;
use crate::render::FrameBuffer;
use crate::render::SynchronizedOutput;

const RESIZE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const ATTACH_TIMEOUT: Duration = Duration::from_secs(2);
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(2);
const AMBIGUOUS_INPUT_TIMEOUT: Duration = Duration::from_millis(50);
const STDIN_BUFFER_SIZE: usize = 8192;
const TABBAR_ROWS: u16 = 1;
const CONTROL_REQUEST_CHANNEL_LIMIT: usize = 128;
const INPUT_REQUEST_CHANNEL_LIMIT: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartResult {
    pub session: SessionName,
    pub socket: PathBuf,
    pub server_pid: ServerPid,
    pub started_server: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PidStatus {
    Malformed { raw: String },
    Missing,
    Running { pid: ServerPid },
    Stale { pid: ServerPid },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StdinRead {
    Bytes(Vec<u8>),
    Eof,
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

/// Start or attach to a muxr session, then detach after validating the handshake.
///
/// # Errors
/// - The session paths cannot be resolved.
/// - The server cannot be started, attached, or detached.
pub fn start_session(session: &SessionName, server_executable: &Path) -> rootcause::Result<StartResult> {
    self::run_async(self::attach_and_detach(session, server_executable))
}

/// Read the muxr server pid file and classify whether that process is usable.
///
/// # Errors
/// - The pid file cannot be read.
/// - The local process table cannot be queried.
pub fn read_pid_status(paths: &SessionPaths) -> rootcause::Result<PidStatus> {
    let raw = match fs::read_to_string(&paths.pid) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(PidStatus::Missing),
        Err(error) => return Err(error).context("failed to read muxr pid file")?,
    };
    let trimmed = raw.trim();
    let Ok(pid) = trimmed.parse::<u32>() else {
        return Ok(PidStatus::Malformed { raw });
    };

    let Ok(pid) = ServerPid::new(pid) else {
        return Ok(PidStatus::Malformed { raw });
    };

    if self::process_exists(pid)? {
        Ok(PidStatus::Running { pid })
    } else {
        Ok(PidStatus::Stale { pid })
    }
}

struct AttachedSession {
    layout: LayoutSnapshot,
    reader: ClientEventReader,
    result: StartResult,
    writer: ClientRequestWriter,
}

enum AttachFailure {
    Rejected(rootcause::Report),
    Unusable(rootcause::Report),
}

impl AttachFailure {
    #[cfg(test)]
    fn into_report(self) -> rootcause::Report {
        match self {
            Self::Rejected(error) | Self::Unusable(error) => error,
        }
    }
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

struct ClientRenderer {
    chrome_dirty: bool,
    frame_buffer: FrameBuffer,
    layout: LayoutSnapshot,
    synchronized_output: SynchronizedOutput,
}

impl ClientRenderer {
    fn new(layout: LayoutSnapshot) -> Self {
        Self::with_synchronized_output(
            layout,
            SynchronizedOutput::for_term(std::env::var("TERM").ok().as_deref()),
        )
    }

    fn with_synchronized_output(layout: LayoutSnapshot, synchronized_output: SynchronizedOutput) -> Self {
        Self {
            chrome_dirty: true,
            frame_buffer: FrameBuffer::default(),
            layout,
            synchronized_output,
        }
    }

    fn apply_layout(&mut self, layout: LayoutSnapshot) {
        // Layout events precede their matching render baseline; defer chrome writes so the user never sees new tab
        // state over an old pane frame.
        self.layout = layout;
        self.chrome_dirty = true;
    }

    fn apply_render(
        &mut self,
        stdout: &mut impl Write,
        update: RenderUpdate,
    ) -> rootcause::Result<ClientRenderOutcome> {
        match self.frame_buffer.apply(update)? {
            ApplyOutcome::Applied(changes) => {
                self.draw(stdout, &changes)?;
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
            crate::tabbar::queue(&mut frame, &self.layout)?;
        }
        self.frame_buffer.queue_at(&mut frame, changes, TABBAR_ROWS)?;
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
}

async fn open_session(
    session: &SessionName,
    terminal_size: TerminalSize,
    server_executable: &Path,
) -> rootcause::Result<AttachedSession> {
    let paths = SessionPaths::from_home(session)?;

    match self::attach(session, &paths, terminal_size.clone()).await {
        Ok(mut attached_session) => {
            attached_session.result.started_server = false;
            return Ok(attached_session);
        }
        Err(attach_failure) => {
            self::handle_attach_failure(&paths, attach_failure).await?;
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
        connection.send_request(&self::hello_request(session, terminal_size)),
    )
    .await
    .map_err(|_| AttachFailure::Unusable(report!("timed out writing muxr attach hello")))?
    .map_err(AttachFailure::Unusable)?;

    let (server_pid, layout) = match tokio::time::timeout(ATTACH_TIMEOUT, connection.recv_event())
        .await
        .map_err(|_| AttachFailure::Unusable(report!("timed out waiting for muxr attach response")))?
        .map_err(AttachFailure::Unusable)?
    {
        Some(ServerEvent::Hello(hello)) => {
            if hello.protocol_version != PROTOCOL_VERSION {
                return Err(AttachFailure::Rejected(
                    report!("muxr server protocol version mismatch")
                        .attach(format!("expected={PROTOCOL_VERSION}"))
                        .attach(format!("actual={}", hello.protocol_version)),
                ));
            }
            (hello.server_pid, hello.layout)
        }
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
        reader,
        result: StartResult {
            session: session.clone(),
            socket: paths.socket.clone(),
            server_pid,
            started_server: false,
        },
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
            Ok(mut attached_session) => {
                attached_session.result.started_server = true;
                return Ok(attached_session);
            }
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

async fn attach_and_detach(session: &SessionName, server_executable: &Path) -> rootcause::Result<StartResult> {
    let attached_session = self::open_session(session, TerminalSize::new(80, 24)?, server_executable).await?;

    self::send_detach_and_wait(attached_session).await
}

async fn send_detach_and_wait(mut attached_session: AttachedSession) -> rootcause::Result<StartResult> {
    attached_session.writer.send_request(&ClientRequest::Detach).await?;

    loop {
        match attached_session.reader.recv_event().await? {
            Some(ServerEvent::Detached) => break,
            Some(ServerEvent::Error(error)) => {
                return Err(report!("muxr server returned error while detaching")
                    .attach(format!("code={}", error.code()))
                    .attach(format!("msg={}", error.msg())));
            }
            Some(ServerEvent::Ping) => attached_session.writer.send_request(&ClientRequest::Pong).await?,
            // Buffered output can legitimately precede the detach ack when a session is reused.
            Some(ServerEvent::Output(_) | ServerEvent::Pong | ServerEvent::Layout(_) | ServerEvent::Render(_)) => {}
            Some(event @ ServerEvent::Hello(_)) => {
                return Err(report!("unexpected muxr server detach event").attach(format!("{event:?}")));
            }
            None => return Err(report!("muxr server closed before detach ack")),
        }
    }

    Ok(attached_session.result)
}

async fn run_interactive(mut attached_session: AttachedSession, initial_size: TerminalSize) -> rootcause::Result<()> {
    let _terminal_guard = TerminalGuard::enable_if_terminal()?;
    let (control_sender, control_receiver) = tokio::sync::mpsc::channel(CONTROL_REQUEST_CHANNEL_LIMIT);
    let (input_sender, input_receiver) = tokio::sync::mpsc::channel(INPUT_REQUEST_CHANNEL_LIMIT);
    let stdin_handle = self::spawn_stdin_forwarder(input_sender);
    let resize_handle = self::spawn_resize_forwarder(control_sender.clone(), initial_size);
    let writer = attached_session.writer;
    let writer_handle =
        tokio::spawn(async move { self::forward_client_requests(writer, control_receiver, input_receiver).await });
    let mut stdout = std::io::stdout();
    let mut renderer = ClientRenderer::new(attached_session.layout);

    while let Some(event) = attached_session.reader.recv_event().await? {
        match event {
            ServerEvent::Detached => break,
            ServerEvent::Error(error) if error.is_command_not_implemented() => {}
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
            ServerEvent::Render(update) => match renderer.apply_render(&mut stdout, update)? {
                ClientRenderOutcome::Drawn => {}
                ClientRenderOutcome::NeedsResync => {
                    if control_sender.send(ClientRequest::RenderResync).await.is_err() {
                        break;
                    }
                }
            },
            ServerEvent::Hello(_) | ServerEvent::Pong => {}
            ServerEvent::Output(bytes) => {
                stdout.write_all(&bytes).context("failed to write muxr pty output")?;
                stdout.flush().context("failed to flush muxr pty output")?;
            }
        }
    }

    writer_handle.abort();
    drop(writer_handle.await);
    drop(stdin_handle);
    drop(resize_handle);
    Ok(())
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

async fn handle_attach_failure(paths: &SessionPaths, attach_failure: AttachFailure) -> rootcause::Result<()> {
    match attach_failure {
        AttachFailure::Rejected(attach_error) => {
            // A structured muxr rejection proves the socket is live even if pid metadata is missing or stale.
            Err(attach_error).attach("socket returned a structured muxr response")
        }
        AttachFailure::Unusable(attach_error) => {
            if self::session_socket_is_live(paths).await? {
                return Err(attach_error).attach("socket responded to muxr liveness probe");
            }
            drop(attach_error);
            Ok(())
        }
    }
}

async fn session_socket_is_live(paths: &SessionPaths) -> rootcause::Result<bool> {
    if !paths.socket.exists() {
        return Ok(false);
    }

    let Ok(mut connection) = self::connect_with_timeout(paths).await else {
        return Ok(false);
    };

    if tokio::time::timeout(ATTACH_TIMEOUT, connection.send_request(&ClientRequest::Ping))
        .await
        .is_err()
    {
        return Ok(false);
    }

    let Ok(Ok(Some(_event))) = tokio::time::timeout(ATTACH_TIMEOUT, connection.recv_event()).await else {
        return Ok(false);
    };

    Ok(true)
}

fn cleanup_stale_session_files(paths: &SessionPaths) -> rootcause::Result<()> {
    // Callers must prove the socket is not a live muxr server before removing files; pid files are only hints.
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

fn process_exists(pid: ServerPid) -> rootcause::Result<bool> {
    let status = Command::new("kill")
        .arg("-0")
        .arg(pid.get().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to query muxr server pid")?;

    Ok(status.success())
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
    let rows = size.rows().saturating_sub(TABBAR_ROWS).max(1);
    TerminalSize::new(size.cols(), rows)
}

fn hello_request(session: &SessionName, terminal_size: TerminalSize) -> ClientRequest {
    ClientRequest::Hello(ClientHello {
        protocol_version: PROTOCOL_VERSION,
        session: session.clone(),
        terminal_size,
    })
}

fn spawn_stdin_forwarder(input_sender: tokio::sync::mpsc::Sender<ClientRequest>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (read_sender, read_receiver) = std::sync::mpsc::channel();
        drop(self::spawn_stdin_reader(read_sender));
        let mut decoder = InputDecoder::default();

        loop {
            // Ambiguous escape prefixes stay buffered across reads; the idle timeout is the interactive path for a bare
            // Esc.
            let read = if decoder.has_pending() {
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
                    drop(input_sender.blocking_send(ClientRequest::Detach));
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

fn send_decoded_input(input_sender: &tokio::sync::mpsc::Sender<ClientRequest>, decoded: Vec<DecodedInput>) -> bool {
    for action in decoded {
        match action {
            DecodedInput::Input(bytes) => {
                if input_sender.blocking_send(ClientRequest::Input(bytes)).is_err() {
                    return false;
                }
            }
            DecodedInput::Key(key) => {
                // Key events come from stdin; keep them on the input queue so raw-byte fallback cannot overtake earlier
                // PTY bytes.
                if input_sender.blocking_send(ClientRequest::Key(key)).is_err() {
                    return false;
                }
            }
            DecodedInput::Paste(bytes) => {
                if input_sender.blocking_send(ClientRequest::Paste(bytes)).is_err() {
                    return false;
                }
            }
            DecodedInput::MouseFocus(position) => {
                let Some(row) = position.row.checked_sub(TABBAR_ROWS) else {
                    continue;
                };
                let request = ClientRequest::FocusPaneAt(muxr_core::ClientMousePosition::new(row, position.col));
                if input_sender.blocking_send(request).is_err() {
                    return false;
                }
            }
            DecodedInput::Scroll { position, direction } => {
                let Some(row) = position.row.checked_sub(TABBAR_ROWS) else {
                    continue;
                };
                let request = ClientRequest::ScrollPaneAt {
                    position: muxr_core::ClientMousePosition::new(row, position.col),
                    direction,
                };
                match input_sender.try_send(request) {
                    Ok(()) => {}
                    Err(tokio::sync::mpsc::error::TrySendError::Full(request)) => drop(request),
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(request)) => {
                        drop(request);
                        return false;
                    }
                }
            }
        }
    }

    true
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
    use muxr_core::ServerError;
    use muxr_core::ServerHello;
    use muxr_transport::ServerListener;
    use rstest::rstest;

    use super::*;

    #[test]
    fn test_read_pid_status_when_pid_file_is_missing_returns_missing() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (_, paths) = self::session_paths(tempdir.path(), "work")?;

        pretty_assertions::assert_eq!(read_pid_status(&paths)?, PidStatus::Missing);
        Ok(())
    }

    #[rstest]
    #[case::not_a_number("abc")]
    #[case::zero("0")]
    fn test_read_pid_status_when_pid_file_is_malformed_returns_malformed(#[case] raw: &str) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (_, paths) = self::session_paths(tempdir.path(), "work")?;
        fs::create_dir_all(&paths.root)?;
        fs::write(&paths.pid, raw)?;

        pretty_assertions::assert_eq!(read_pid_status(&paths)?, PidStatus::Malformed { raw: raw.to_owned() },);
        Ok(())
    }

    #[test]
    fn test_read_pid_status_when_pid_file_process_is_missing_returns_stale() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (_, paths) = self::session_paths(tempdir.path(), "work")?;
        fs::create_dir_all(&paths.root)?;
        fs::write(&paths.pid, u32::MAX.to_string())?;

        pretty_assertions::assert_eq!(
            read_pid_status(&paths)?,
            PidStatus::Stale {
                pid: ServerPid::new(u32::MAX)?
            },
        );
        Ok(())
    }

    #[test]
    fn test_read_pid_status_when_pid_file_process_is_running_returns_running() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (_, paths) = self::session_paths(tempdir.path(), "work")?;
        fs::create_dir_all(&paths.root)?;
        fs::write(&paths.pid, std::process::id().to_string())?;

        pretty_assertions::assert_eq!(
            read_pid_status(&paths)?,
            PidStatus::Running {
                pid: ServerPid::new(std::process::id())?
            },
        );
        Ok(())
    }

    #[test]
    fn test_cleanup_stale_session_files_when_running_pid_has_missing_socket_removes_pid() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (_, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            fs::write(&paths.pid, std::process::id().to_string())?;

            handle_attach_failure(&paths, AttachFailure::Unusable(report!("connect failed"))).await?;
            cleanup_stale_session_files(&paths)?;

            assert2::assert!(!paths.pid.exists());
            assert2::assert!(!paths.socket.exists());
            Ok(())
        })
    }

    #[test]
    fn test_attach_when_pid_file_is_missing_after_hello_uses_server_pid_from_hello() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let listener = ServerListener::bind(&paths.socket)?;
            let server_session = session.clone();
            let handle = tokio::spawn(async move {
                let mut connection = listener.accept().await?;
                assert2::assert!(matches!(
                    connection.recv_request().await?,
                    Some(ClientRequest::Hello(_))
                ));
                connection
                    .send_event(&ServerEvent::Hello(ServerHello {
                        protocol_version: PROTOCOL_VERSION,
                        session: server_session,
                        server_pid: ServerPid::new(4242)?,
                        layout: self::layout_snapshot()?,
                    }))
                    .await?;
                Ok::<(), rootcause::Report>(())
            });

            let attached_session = attach(&session, &paths, TerminalSize::new(80, 24)?)
                .await
                .map_err(AttachFailure::into_report)?;

            pretty_assertions::assert_eq!(attached_session.result.server_pid, ServerPid::new(4242)?);
            handle
                .await
                .map_err(|error| report!("muxr client test socket task panicked").attach(format!("{error}")))??;
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

            assert2::assert!(
                handle_attach_failure(&paths, AttachFailure::Rejected(report!("already attached")))
                    .await
                    .is_err()
            );
            assert2::assert!(paths.socket.exists());
            Ok(())
        })
    }

    #[test]
    fn test_session_socket_is_live_when_server_replies_pong_returns_true() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (_, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let listener = ServerListener::bind(&paths.socket)?;
            let handle = tokio::spawn(async move {
                let mut connection = listener.accept().await?;
                assert2::assert!(matches!(connection.recv_request().await?, Some(ClientRequest::Ping)));
                connection.send_event(&ServerEvent::Pong).await?;
                Ok::<(), rootcause::Report>(())
            });

            pretty_assertions::assert_eq!(session_socket_is_live(&paths).await?, true);
            handle
                .await
                .map_err(|error| report!("muxr liveness test socket task panicked").attach(format!("{error}")))??;
            Ok(())
        })
    }

    #[test]
    fn test_session_socket_is_live_when_socket_is_missing_returns_false() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (_, paths) = self::session_paths(tempdir.path(), "work")?;

            pretty_assertions::assert_eq!(session_socket_is_live(&paths).await?, false);
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
                    Some(ClientRequest::Hello(_))
                ));
                connection
                    .send_event(&ServerEvent::Error(ServerError::client_already_attached()))
                    .await?;
                Ok::<(), rootcause::Report>(())
            });

            let attach_error = attach(&session, &paths, TerminalSize::new(80, 24)?)
                .await
                .map_or_else(AttachFailure::into_report, |_| report!("expected rejected attach"));

            assert2::assert!(attach_error.to_string().contains("muxr server rejected attach"));
            handle
                .await
                .map_err(|error| report!("muxr rejected attach test task panicked").attach(format!("{error}")))??;
            Ok(())
        })
    }

    #[test]
    fn test_attach_and_detach_when_output_and_ping_precede_detached_returns_result() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let listener = ServerListener::bind(&paths.socket)?;
            let server_session = session.clone();
            let handle = tokio::spawn(async move {
                let mut connection = listener.accept().await?;
                assert2::assert!(matches!(
                    connection.recv_request().await?,
                    Some(ClientRequest::Hello(_))
                ));
                connection
                    .send_event(&ServerEvent::Hello(ServerHello {
                        protocol_version: PROTOCOL_VERSION,
                        session: server_session,
                        server_pid: ServerPid::new(4242)?,
                        layout: self::layout_snapshot()?,
                    }))
                    .await?;
                connection
                    .send_event(&ServerEvent::Output(b"buffered".to_vec()))
                    .await?;
                connection.send_event(&ServerEvent::Ping).await?;

                let mut saw_detach = false;
                let mut saw_pong = false;
                for _ in 0..2 {
                    match connection.recv_request().await? {
                        Some(ClientRequest::Detach) => saw_detach = true,
                        Some(ClientRequest::Pong) => saw_pong = true,
                        Some(request) => {
                            return Err(report!("unexpected muxr detach test request").attach(format!("{request:?}")));
                        }
                        None => return Err(report!("muxr detach test client closed early")),
                    }
                }
                assert2::assert!(saw_detach);
                assert2::assert!(saw_pong);
                connection.send_event(&ServerEvent::Detached).await?;
                Ok::<(), rootcause::Report>(())
            });

            let attached_session = attach(&session, &paths, TerminalSize::new(80, 24)?)
                .await
                .map_err(AttachFailure::into_report)?;
            let result = send_detach_and_wait(attached_session).await?;

            pretty_assertions::assert_eq!(result.server_pid, ServerPid::new(4242)?);
            handle
                .await
                .map_err(|error| report!("muxr detach test socket task panicked").attach(format!("{error}")))??;
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
            Some(ClientRequest::Input(b"a".to_vec())),
        );
        pretty_assertions::assert_eq!(input_receiver.blocking_recv(), Some(ClientRequest::Key(key)),);
        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientRequest::Input(b"b".to_vec())),
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
            Some(ClientRequest::Paste(b"one\ntwo\n".to_vec())),
        );
    }

    #[test]
    fn test_send_decoded_input_when_mouse_focus_arrives_offsets_tabbar_row() {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);

        assert2::assert!(send_decoded_input(
            &input_sender,
            vec![DecodedInput::MouseFocus(muxr_core::ClientMousePosition::new(4, 9))],
        ));

        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientRequest::FocusPaneAt(muxr_core::ClientMousePosition::new(3, 9))),
        );
    }

    #[test]
    fn test_send_decoded_input_when_mouse_focus_is_on_tabbar_drops_request() {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);

        assert2::assert!(send_decoded_input(
            &input_sender,
            vec![DecodedInput::MouseFocus(muxr_core::ClientMousePosition::new(0, 9))],
        ));

        assert2::assert!(matches!(
            input_receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn test_send_decoded_input_when_scroll_queue_is_full_drops_scroll() {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
        assert2::assert!(input_sender.try_send(ClientRequest::Input(b"pending".to_vec())).is_ok());

        assert2::assert!(send_decoded_input(
            &input_sender,
            vec![DecodedInput::Scroll {
                position: muxr_core::ClientMousePosition::new(4, 9),
                direction: muxr_core::PaneScrollDirection::Up,
            }],
        ));

        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientRequest::Input(b"pending".to_vec())),
        );
        assert2::assert!(matches!(
            input_receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn test_send_decoded_input_when_scroll_arrives_offsets_tabbar_row() {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);

        assert2::assert!(send_decoded_input(
            &input_sender,
            vec![DecodedInput::Scroll {
                position: muxr_core::ClientMousePosition::new(4, 9),
                direction: muxr_core::PaneScrollDirection::Up,
            }],
        ));

        pretty_assertions::assert_eq!(
            input_receiver.blocking_recv(),
            Some(ClientRequest::ScrollPaneAt {
                position: muxr_core::ClientMousePosition::new(3, 9),
                direction: muxr_core::PaneScrollDirection::Up,
            }),
        );
    }

    #[test]
    fn test_send_decoded_input_when_scroll_is_on_tabbar_drops_request() {
        let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);

        assert2::assert!(send_decoded_input(
            &input_sender,
            vec![DecodedInput::Scroll {
                position: muxr_core::ClientMousePosition::new(0, 9),
                direction: muxr_core::PaneScrollDirection::Up,
            }],
        ));

        assert2::assert!(matches!(
            input_receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn test_pane_size_for_terminal_when_tabbar_has_room_reserves_one_row() -> rootcause::Result<()> {
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
        let mut renderer = ClientRenderer::with_synchronized_output(layout_snapshot()?, SynchronizedOutput::Csi);
        let output = CountingWriter::default();

        renderer.apply_layout(two_tab_layout()?);

        pretty_assertions::assert_eq!(output.bytes, Vec::<u8>::new());
        pretty_assertions::assert_eq!(output.flushes, 0);
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_render_when_layout_is_dirty_flushes_one_complete_frame() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(layout_snapshot()?, SynchronizedOutput::Csi);
        renderer.apply_layout(two_tab_layout()?);
        let mut output = CountingWriter::default();

        let outcome = renderer.apply_render(&mut output, muxr_core::RenderUpdate::Baseline(render_baseline()?))?;

        pretty_assertions::assert_eq!(outcome, ClientRenderOutcome::Drawn);
        pretty_assertions::assert_eq!(output.flushes, 1);
        let terminal_output = output.rendered_string()?;
        assert2::assert!(terminal_output.starts_with("\x1b[?2026h"));
        assert2::assert!(terminal_output.ends_with("\x1b[?2026l"));
        let clear_index = terminal_output.find("\x1b[2J").unwrap_or(usize::MAX);
        let tabbar_index = terminal_output.find("[2:tab 2]").unwrap_or(usize::MAX);
        let pane_index = terminal_output.find("ab").unwrap_or(usize::MAX);
        assert2::assert!(clear_index < tabbar_index);
        assert2::assert!(tabbar_index < pane_index);
        Ok(())
    }

    #[test]
    fn test_client_renderer_apply_render_when_resync_is_needed_does_not_flush() -> rootcause::Result<()> {
        let mut renderer = ClientRenderer::with_synchronized_output(layout_snapshot()?, SynchronizedOutput::Csi);
        let mut output = CountingWriter::default();

        let outcome = renderer.apply_render(&mut output, muxr_core::RenderUpdate::Diff(render_diff()?))?;

        pretty_assertions::assert_eq!(outcome, ClientRenderOutcome::NeedsResync);
        pretty_assertions::assert_eq!(output.bytes, Vec::<u8>::new());
        pretty_assertions::assert_eq!(output.flushes, 0);
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
        LayoutSnapshot::single_pane("tab-1", "default", "pane-1", "shell")
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
            )],
        )
    }

    fn render_diff() -> rootcause::Result<muxr_core::RenderDiff> {
        muxr_core::RenderDiff::new(
            1,
            2,
            TerminalSize::new(2, 1)?,
            muxr_core::RenderCursor::new(0, 1, true),
            vec![muxr_core::RenderRowSpan::new(0, 0, vec![render_cell("x")])],
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
