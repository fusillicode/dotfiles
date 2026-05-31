use std::env;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::mpsc::TrySendError;
use std::thread;

use muxr_core::PaneScrollDirection;
use muxr_core::TerminalSize;
use portable_pty::Child;
use portable_pty::CommandBuilder;
use portable_pty::ExitStatus;
use portable_pty::MasterPty;
use portable_pty::PtySize;
use portable_pty::native_pty_system;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

use crate::history::PaneHistory;
use crate::terminal::TerminalSnapshot;
use crate::terminal::TerminalState;

const READ_BUFFER_SIZE: usize = 8192;
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellCommand {
    program: PathBuf,
    args: Vec<String>,
}

impl ShellCommand {
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    #[must_use]
    #[cfg(test)]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn default_from_env() -> Self {
        let program = env::var_os("SHELL")
            .filter(|value| !value.as_os_str().is_empty())
            .map_or_else(default_shell_path, PathBuf::from);

        Self::new(program)
    }

    #[must_use]
    pub fn label(&self) -> String {
        self.program
            .file_name()
            .and_then(|name| name.to_str())
            .map_or_else(|| self.program.to_string_lossy().into_owned(), ToOwned::to_owned)
    }

    fn command_builder(&self) -> CommandBuilder {
        let mut command = CommandBuilder::new(self.program.as_os_str());
        for arg in &self.args {
            command.arg(arg);
        }
        command
    }
}

#[derive(Debug)]
pub enum PtyEvent {
    Exited,
    OutputReady,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PtyExitStatus {
    pub code: u32,
    pub signal: Option<String>,
    pub success: bool,
}

pub struct PtySession {
    handle: PtyHandle,
    reader_handle: Option<thread::JoinHandle<()>>,
}

impl PtySession {
    pub fn spawn(command: &ShellCommand, size: &TerminalSize, history_path: &Path) -> rootcause::Result<Self> {
        let state = Arc::new(PtyState::with_history(size, history_path)?);
        let pty_pair = native_pty_system()
            .openpty(pty_size(size))
            .map_err(|error| report!("failed to open muxr shell pty").attach(format!("error={error:#}")))?;
        let child = pty_pair
            .slave
            .spawn_command(command.command_builder())
            .map_err(|error| report!("failed to spawn muxr shell process").attach(format!("error={error:#}")))?;
        let reader = pty_pair
            .master
            .try_clone_reader()
            .map_err(|error| report!("failed to clone muxr pty reader").attach(format!("error={error:#}")))?;
        let writer = pty_pair
            .master
            .take_writer()
            .map_err(|error| report!("failed to take muxr pty writer").attach(format!("error={error:#}")))?;
        drop(pty_pair.slave);

        let writer = Arc::new(Mutex::new(writer));
        let handle = PtyHandle {
            child: Arc::new(Mutex::new(child)),
            master: Arc::new(Mutex::new(pty_pair.master)),
            state: Arc::clone(&state),
            writer: Arc::clone(&writer),
        };
        let reader_handle = Some(spawn_reader_thread(reader, state, writer));

        Ok(Self { handle, reader_handle })
    }

    pub fn handle(&self) -> PtyHandle {
        self.handle.clone()
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        if !self.handle.state.exited.load(Ordering::Acquire)
            && let Ok(mut child) = self.handle.child.lock()
        {
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    drop(self.handle.state.mark_exited(PtyExitStatus::from(&exit_status)));
                }
                Ok(None) => {
                    drop(child.kill());
                    if let Ok(exit_status) = child.wait() {
                        drop(self.handle.state.mark_exited(PtyExitStatus::from(&exit_status)));
                    }
                }
                Err(_) => {
                    drop(child.kill());
                }
            }
        }

        if let Some(reader_handle) = self.reader_handle.take() {
            drop(reader_handle.join());
        }
    }
}

#[derive(Clone)]
pub struct PtyHandle {
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    state: Arc<PtyState>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl PtyHandle {
    pub fn attach_sink(&self, sender: mpsc::SyncSender<PtyEvent>) -> rootcause::Result<PtySinkGuard> {
        self.state.attach_sink(sender)
    }

    pub fn has_exited(&self) -> rootcause::Result<bool> {
        if self.state.exited.load(Ordering::Acquire) {
            return Ok(true);
        }

        let exit_status = {
            let mut child = lock_mutex(&self.child, "pty child")?;
            child.try_wait().context("failed to poll muxr shell process")?
        };
        if let Some(exit_status) = exit_status {
            self.state.mark_exited(PtyExitStatus::from(&exit_status))?;
            return Ok(true);
        }

        Ok(false)
    }

    pub fn resize(&self, size: &TerminalSize) -> rootcause::Result<()> {
        lock_mutex(&self.master, "pty master")?
            .resize(pty_size(size))
            .map_err(|error| report!("failed to resize muxr shell pty").attach(format!("error={error:#}")))?;
        lock_mutex(&self.state.terminal, "pty terminal")?.resize(size);
        Ok(())
    }

    pub fn write_input(&self, bytes: &[u8]) -> rootcause::Result<bool> {
        if bytes.is_empty() {
            return Ok(false);
        }

        // PTY-bound input should reveal the live viewport before an app echoes typed bytes; some apps do not echo, so
        // callers need the changed flag to redraw immediately after resetting scrollback.
        let scrolled_to_bottom = lock_mutex(&self.state.terminal, "pty terminal")?.scroll_to_bottom();
        self::write_pty_bytes(
            self.writer.as_ref(),
            bytes,
            "failed to write client input to muxr shell pty",
            "failed to flush muxr shell pty input",
        )?;
        Ok(scrolled_to_bottom)
    }

    pub fn write_paste(&self, bytes: &[u8]) -> rootcause::Result<bool> {
        if bytes.is_empty() {
            return Ok(false);
        }

        let (scrolled_to_bottom, bracketed_paste_enabled) = {
            let mut terminal = lock_mutex(&self.state.terminal, "pty terminal")?;
            let scrolled_to_bottom = terminal.scroll_to_bottom();
            (scrolled_to_bottom, terminal.bracketed_paste_enabled())
        };
        let framed = self::pty_paste_bytes(bytes, bracketed_paste_enabled);
        self::write_pty_bytes(
            self.writer.as_ref(),
            &framed,
            "failed to write client paste to muxr shell pty",
            "failed to flush muxr shell pty paste",
        )?;
        Ok(scrolled_to_bottom)
    }

    pub fn scroll(&self, direction: PaneScrollDirection) -> rootcause::Result<bool> {
        Ok(lock_mutex(&self.state.terminal, "pty terminal")?.scroll(direction))
    }

    pub fn exit_status(&self) -> rootcause::Result<Option<PtyExitStatus>> {
        let stored_exit_status = lock_mutex(&self.state.exit_status, "pty exit status")?.clone();
        if let Some(exit_status) = stored_exit_status {
            return Ok(Some(exit_status));
        }

        let exit_status = {
            let mut child = lock_mutex(&self.child, "pty child")?;
            child.try_wait().context("failed to poll muxr shell process")?
        };

        let Some(exit_status) = exit_status else {
            return Ok(None);
        };

        let exit_status = PtyExitStatus::from(&exit_status);
        self.state.mark_exited(exit_status.clone())?;
        Ok(Some(exit_status))
    }

    pub fn render_snapshot(&self) -> rootcause::Result<TerminalSnapshot> {
        lock_mutex(&self.state.terminal, "pty terminal")?.snapshot()
    }
}

pub struct PtySinkGuard {
    output_current: Arc<AtomicBool>,
    state: Arc<PtyState>,
}

impl PtySinkGuard {
    /// Return false after the live output sink overflows or disconnects.
    pub fn is_output_current(&self) -> bool {
        self.output_current.load(Ordering::Acquire)
    }
}

impl Drop for PtySinkGuard {
    fn drop(&mut self) {
        if let Ok(mut active_sink) = self.state.active_sink.lock() {
            *active_sink = None;
        }
    }
}

struct ActivePtySink {
    output_current: Arc<AtomicBool>,
    sender: mpsc::SyncSender<PtyEvent>,
}

struct PtyState {
    active_sink: Mutex<Option<ActivePtySink>>,
    exited: AtomicBool,
    exit_status: Mutex<Option<PtyExitStatus>>,
    history: Mutex<Option<PaneHistory>>,
    terminal: Mutex<TerminalState>,
}

impl PtyState {
    #[cfg(test)]
    fn new(size: &TerminalSize) -> Self {
        Self {
            active_sink: Mutex::new(None),
            exited: AtomicBool::new(false),
            exit_status: Mutex::new(None),
            history: Mutex::new(None),
            terminal: Mutex::new(TerminalState::new(size)),
        }
    }

    fn with_history(size: &TerminalSize, history_path: &Path) -> rootcause::Result<Self> {
        let (history, replay) = PaneHistory::open(history_path)?;
        let mut terminal = TerminalState::new(size);
        drop(terminal.process(&replay));

        Ok(Self {
            active_sink: Mutex::new(None),
            exited: AtomicBool::new(false),
            exit_status: Mutex::new(None),
            history: Mutex::new(Some(history)),
            terminal: Mutex::new(terminal),
        })
    }

    fn attach_sink(self: &Arc<Self>, sender: mpsc::SyncSender<PtyEvent>) -> rootcause::Result<PtySinkGuard> {
        let output_current = Arc::new(AtomicBool::new(true));
        *lock_mutex(&self.active_sink, "pty active sink")? = Some(ActivePtySink {
            output_current: Arc::clone(&output_current),
            sender,
        });

        Ok(PtySinkGuard {
            output_current,
            state: Arc::clone(self),
        })
    }

    fn append_output(&self, bytes: &[u8]) -> rootcause::Result<Vec<Vec<u8>>> {
        if let Some(history) = lock_mutex(&self.history, "pty history")?.as_mut() {
            history.append(bytes)?;
        }
        let terminal_replies = lock_mutex(&self.terminal, "pty terminal")?.process(bytes);

        let mut active_sink = lock_mutex(&self.active_sink, "pty active sink")?;
        if let Some(sink) = active_sink.as_ref() {
            match sink.sender.try_send(PtyEvent::OutputReady) {
                Ok(()) | Err(TrySendError::Full(PtyEvent::OutputReady)) => {}
                Err(TrySendError::Disconnected(PtyEvent::OutputReady)) => {
                    sink.output_current.store(false, Ordering::Release);
                    *active_sink = None;
                }
                Err(TrySendError::Full(PtyEvent::Exited) | TrySendError::Disconnected(PtyEvent::Exited)) => {
                    return Err(report!("unexpected muxr pty exit event while sending output"));
                }
            }
        }
        drop(active_sink);

        Ok(terminal_replies)
    }

    fn mark_exited(&self, exit_status: PtyExitStatus) -> rootcause::Result<()> {
        let mut stored_exit_status = lock_mutex(&self.exit_status, "pty exit status")?;
        if stored_exit_status.is_none() {
            *stored_exit_status = Some(exit_status);
        }
        drop(stored_exit_status);

        self.exited.store(true, Ordering::Release);

        let mut active_sink = lock_mutex(&self.active_sink, "pty active sink")?;
        if let Some(sink) = active_sink.as_ref()
            && sink.sender.try_send(PtyEvent::Exited).is_err()
        {
            sink.output_current.store(false, Ordering::Release);
            *active_sink = None;
        }
        drop(active_sink);

        Ok(())
    }
}

impl From<&ExitStatus> for PtyExitStatus {
    fn from(status: &ExitStatus) -> Self {
        Self {
            code: status.exit_code(),
            signal: status.signal().map(ToOwned::to_owned),
            success: status.success(),
        }
    }
}

fn default_shell_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/bin/zsh")
    } else {
        PathBuf::from("/bin/sh")
    }
}

fn lock_mutex<'a, T>(mutex: &'a Mutex<T>, name: &str) -> rootcause::Result<MutexGuard<'a, T>> {
    mutex.lock().map_err(|_| report!("poisoned muxr {name} mutex"))
}

fn write_pty_bytes(
    writer: &Mutex<Box<dyn Write + Send>>,
    bytes: &[u8],
    write_context: &'static str,
    flush_context: &'static str,
) -> rootcause::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }

    let mut writer = self::lock_mutex(writer, "pty writer")?;
    writer.write_all(bytes).context(write_context)?;
    writer.flush().context(flush_context)?;
    drop(writer);
    Ok(())
}

fn pty_paste_bytes(bytes: &[u8], bracketed_paste_enabled: bool) -> Vec<u8> {
    if !bracketed_paste_enabled {
        return bytes.to_vec();
    }

    let mut framed = Vec::with_capacity(
        BRACKETED_PASTE_START
            .len()
            .saturating_add(bytes.len())
            .saturating_add(BRACKETED_PASTE_END.len()),
    );
    framed.extend_from_slice(BRACKETED_PASTE_START);
    framed.extend_from_slice(bytes);
    framed.extend_from_slice(BRACKETED_PASTE_END);
    framed
}

fn write_terminal_replies(writer: &Mutex<Box<dyn Write + Send>>, replies: &[Vec<u8>]) -> rootcause::Result<()> {
    if replies.is_empty() {
        return Ok(());
    }

    let mut writer = self::lock_mutex(writer, "pty writer")?;
    for reply in replies {
        writer
            .write_all(reply.as_slice())
            .context("failed to write muxr terminal reply to shell pty")?;
    }
    writer
        .flush()
        .context("failed to flush muxr terminal reply to shell pty")?;
    drop(writer);
    Ok(())
}

const fn pty_size(size: &TerminalSize) -> PtySize {
    PtySize {
        rows: size.rows(),
        cols: size.cols(),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn spawn_reader_thread(
    mut reader: Box<dyn Read + Send>,
    state: Arc<PtyState>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0; READ_BUFFER_SIZE];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => {
                    // PTY EOF only means the slave side closed; the child may still be running
                    // after redirecting stdio, so only child polling is allowed to mark exit.
                    break;
                }
                Ok(bytes_read) => {
                    let Some(bytes) = buffer.get(..bytes_read) else {
                        break;
                    };
                    let Ok(terminal_replies) = state.append_output(bytes) else {
                        break;
                    };
                    if self::write_terminal_replies(writer.as_ref(), &terminal_replies).is_err() {
                        break;
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_reader_thread_when_pty_reaches_eof_does_not_mark_child_exited() -> rootcause::Result<()> {
        let state = Arc::new(PtyState::new(&terminal_size()?));
        let reader_handle = spawn_reader_thread(
            Box::new(std::io::Cursor::new(Vec::new())),
            Arc::clone(&state),
            self::sink_pty_writer(),
        );

        reader_handle
            .join()
            .map_err(|_| report!("muxr pty reader test thread panicked"))?;

        assert2::assert!(!state.exited.load(Ordering::Acquire));
        assert2::assert!(lock_mutex(&state.exit_status, "pty exit status")?.is_none());
        Ok(())
    }

    #[test]
    fn test_attach_sink_when_output_arrives_after_attach_delivers_live_event() -> rootcause::Result<()> {
        let state = Arc::new(PtyState::new(&terminal_size()?));
        pretty_assertions::assert_eq!(state.append_output(b"before")?, Vec::<Vec<u8>>::new());
        let (sender, receiver) = mpsc::sync_channel(1);

        let _guard = state.attach_sink(sender)?;
        pretty_assertions::assert_eq!(state.append_output(b"after")?, Vec::<Vec<u8>>::new());

        assert2::assert!(matches!(receiver.recv(), Ok(PtyEvent::OutputReady)));
        Ok(())
    }

    #[test]
    fn test_append_output_when_sink_is_full_coalesces_without_blocking() -> rootcause::Result<()> {
        let state = PtyState::new(&terminal_size()?);
        let (sender, receiver) = mpsc::sync_channel(1);
        let output_current = Arc::new(AtomicBool::new(true));
        *lock_mutex(&state.active_sink, "pty active sink")? = Some(ActivePtySink {
            output_current: Arc::clone(&output_current),
            sender,
        });

        pretty_assertions::assert_eq!(state.append_output(b"first")?, Vec::<Vec<u8>>::new());
        pretty_assertions::assert_eq!(state.append_output(b"second")?, Vec::<Vec<u8>>::new());

        assert2::assert!(lock_mutex(&state.active_sink, "pty active sink")?.is_some());
        assert2::assert!(output_current.load(Ordering::Acquire));
        assert2::assert!(matches!(receiver.recv(), Ok(PtyEvent::OutputReady)));
        Ok(())
    }

    #[test]
    fn test_append_output_when_terminal_reply_is_generated_writes_reply_to_pty() -> rootcause::Result<()> {
        let state = PtyState::new(&terminal_size()?);
        let written = Arc::new(Mutex::new(Vec::new()));
        let writer = self::capturing_pty_writer(Arc::clone(&written));

        let replies = state.append_output(b"\x1b[6n")?;
        self::write_terminal_replies(writer.as_ref(), &replies)?;

        pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, b"\x1b[1;1R".to_vec());
        Ok(())
    }

    #[test]
    fn test_pty_state_with_history_when_output_exists_replays_terminal_state() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let path = tempdir.path().join("pane-1").join("output.raw");
        std::fs::create_dir_all(path.parent().ok_or_else(|| report!("expected history parent"))?)?;
        std::fs::write(&path, b"history").context("failed to write muxr test history")?;

        let state = PtyState::with_history(&terminal_size()?, &path)?;
        let snapshot = lock_mutex(&state.terminal, "pty terminal")?.snapshot()?;
        let rendered = snapshot
            .rows()
            .iter()
            .flat_map(|row| row.cells.iter().map(|cell| cell.text.as_str()))
            .collect::<String>();

        assert2::assert!(rendered.contains("history"));
        Ok(())
    }

    #[test]
    fn test_pty_paste_bytes_when_bracketed_paste_is_enabled_wraps_payload() {
        pretty_assertions::assert_eq!(
            pty_paste_bytes(b"one\ntwo\n", true),
            b"\x1b[200~one\ntwo\n\x1b[201~".to_vec(),
        );
    }

    #[test]
    fn test_pty_paste_bytes_when_bracketed_paste_is_disabled_preserves_payload() {
        pretty_assertions::assert_eq!(pty_paste_bytes(b"one\ntwo\n", false), b"one\ntwo\n".to_vec());
    }

    fn terminal_size() -> rootcause::Result<TerminalSize> {
        TerminalSize::new(80, 24)
    }

    fn sink_pty_writer() -> Arc<Mutex<Box<dyn Write + Send>>> {
        Arc::new(Mutex::new(Box::new(std::io::sink())))
    }

    fn capturing_pty_writer(written: Arc<Mutex<Vec<u8>>>) -> Arc<Mutex<Box<dyn Write + Send>>> {
        Arc::new(Mutex::new(Box::new(CapturingWriter { written })))
    }

    fn captured_pty_bytes(written: &Mutex<Vec<u8>>) -> rootcause::Result<Vec<u8>> {
        Ok(lock_mutex(written, "captured pty bytes")?.clone())
    }

    struct CapturingWriter {
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for CapturingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut written = self
                .written
                .lock()
                .map_err(|_| std::io::Error::other("poisoned muxr capturing writer"))?;
            written.extend_from_slice(buf);
            drop(written);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
