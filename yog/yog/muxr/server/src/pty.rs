use std::env;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::mpsc::TrySendError;
use std::thread;

use muxr_config::ScrollbackConfig;
use muxr_config::ScrollbackDumpStyle;
use muxr_core::ClientMouseEvent;
use muxr_core::PaneMouseMode;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneScrollDirection;
use muxr_core::TerminalSize;
use portable_pty::Child;
use portable_pty::ChildKiller;
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
use crate::terminal::TerminalApplicationMode;
use crate::terminal::TerminalCursorKeyMode;
use crate::terminal::TerminalFocusEvent;
use crate::terminal::TerminalFocusReporting;
use crate::terminal::TerminalMouseProtocol;
use crate::terminal::TerminalSnapshot;
use crate::terminal::TerminalState;

const READ_BUFFER_SIZE: usize = 8192;
const PTY_WRITE_QUEUE_LIMIT: usize = 1024;
const PTY_WRITE_QUEUE_BYTE_LIMIT: usize = 1024 * 1024;
const PTY_WRITE_BATCH_MAX_MESSAGES: usize = 64;
const PTY_WRITE_BATCH_MAX_BYTES: usize = 64 * 1024;
const PTY_WRITE_MAX_MESSAGE_BYTES: usize = PTY_WRITE_BATCH_MAX_BYTES;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellCmd {
    program: PathBuf,
    args: Vec<String>,
}

impl ShellCmd {
    /// Build a shell cmd for a muxr pane.
    ///
    /// # Errors
    /// - The program path is empty.
    pub fn new(program: impl Into<PathBuf>) -> rootcause::Result<Self> {
        let program = program.into();
        if program.as_os_str().is_empty() {
            return Err(report!("invalid muxr shell cmd").attach("reason=program path must not be empty"));
        }

        Ok(Self {
            program,
            args: Vec::new(),
        })
    }

    /// Build a pane startup cmd with arguments.
    ///
    /// # Errors
    /// - The program path is empty.
    pub fn with_args(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> rootcause::Result<Self> {
        let mut cmd = Self::new(program)?;
        cmd.args = args.into_iter().map(Into::into).collect();
        Ok(cmd)
    }

    /// Build the default shell cmd from `$SHELL`, falling back to `/bin/sh`.
    ///
    /// # Errors
    /// - The selected program path is empty.
    pub fn default_from_env() -> rootcause::Result<Self> {
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

    #[must_use]
    pub fn label_with_args(&self) -> String {
        let mut label = self.label();
        for arg in &self.args {
            label.push(' ');
            label.push_str(arg);
        }
        label
    }

    #[must_use]
    pub fn shell_input_line(&self) -> String {
        let mut line = self::shell_quote(&self.program.to_string_lossy());
        for arg in &self.args {
            line.push(' ');
            line.push_str(&self::shell_quote(arg));
        }
        line.push('\n');
        line
    }

    fn cmd_builder(&self, cwd: &str) -> rootcause::Result<CommandBuilder> {
        let mut cmd = CommandBuilder::new(self.program.as_os_str());
        cmd.cwd(self::resolved_cwd(cwd)?);
        for arg in &self.args {
            cmd.arg(arg);
        }
        Ok(cmd)
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
    child_wait_handle: Option<thread::JoinHandle<()>>,
    handle: PtyHandle,
    reader_handle: Option<thread::JoinHandle<()>>,
    writer_handle: Option<thread::JoinHandle<()>>,
}

impl PtySession {
    pub fn spawn(
        cmd: &ShellCmd,
        cwd: &str,
        size: &TerminalSize,
        history_path: &Path,
        scrollback: ScrollbackConfig,
        pane_exit_notify: Arc<tokio::sync::Notify>,
    ) -> rootcause::Result<Self> {
        let state = Arc::new(PtyState::with_history(
            size,
            history_path,
            scrollback,
            pane_exit_notify,
        )?);
        let pty_pair = native_pty_system()
            .openpty(pty_size(size))
            .map_err(|error| report!("failed to open muxr shell pty").attach(format!("error={error:#}")))?;
        let child = pty_pair
            .slave
            .spawn_command(cmd.cmd_builder(cwd)?)
            .map_err(|error| report!("failed to spawn muxr shell process").attach(format!("error={error:#}")))?;
        let child_process_id = child.process_id();
        let child_killer = Arc::new(Mutex::new(child.clone_killer()));
        let reader = pty_pair
            .master
            .try_clone_reader()
            .map_err(|error| report!("failed to clone muxr pty reader").attach(format!("error={error:#}")))?;
        let writer = pty_pair
            .master
            .take_writer()
            .map_err(|error| report!("failed to take muxr pty writer").attach(format!("error={error:#}")))?;
        drop(pty_pair.slave);

        let (writer, writer_handle) = self::spawn_writer_thread(writer);
        let handle = PtyHandle {
            child_killer,
            child_process_id,
            master: Arc::new(Mutex::new(pty_pair.master)),
            state: Arc::clone(&state),
            writer: writer.clone(),
        };
        let child_wait_handle = Some(spawn_child_wait_thread(child, Arc::clone(&state)));
        let reader_handle = Some(spawn_reader_thread(reader, state, writer));

        Ok(Self {
            child_wait_handle,
            handle,
            reader_handle,
            writer_handle: Some(writer_handle),
        })
    }

    pub fn handle(&self) -> PtyHandle {
        self.handle.clone()
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        if !self.handle.state.exited.load(Ordering::Acquire) {
            match self::lock_mutex(&self.handle.child_killer, "pty child killer") {
                Ok(mut killer) => {
                    let _ = killer.kill().inspect_err(|error| {
                        crate::session::tracing::pty::shutdown_failed("kill_child", error);
                    });
                }
                Err(error) => {
                    crate::session::tracing::pty::shutdown_failed("lock_child_killer", &error);
                }
            }
        }

        if let Some(child_wait_handle) = self.child_wait_handle.take()
            && child_wait_handle.join().is_err()
        {
            crate::session::tracing::pty::shutdown_failed("join_child_wait", "child wait thread panicked");
        }
        let writer_shutdown = self
            .handle
            .writer
            .shutdown()
            .inspect_err(|error| {
                crate::session::tracing::pty::shutdown_failed("queue_writer_shutdown", error);
            })
            .is_ok();
        if let Some(reader_handle) = self.reader_handle.take()
            && reader_handle.join().is_err()
        {
            crate::session::tracing::pty::shutdown_failed("join_reader", "reader thread panicked");
        }
        if writer_shutdown
            && let Some(writer_handle) = self.writer_handle.take()
            && writer_handle.join().is_err()
        {
            crate::session::tracing::pty::shutdown_failed("join_writer", "writer thread panicked");
        }
    }
}

#[derive(Clone)]
struct PtyWriteQueue {
    sender: mpsc::SyncSender<PtyWriteRequest>,
    state: Arc<PtyWriteState>,
}

impl PtyWriteQueue {
    fn enqueue(&self, mut write: PtyWrite) -> rootcause::Result<()> {
        let write_len = write.len();
        let mut queue_guard = self::lock_mutex(&self.state.queue, "pty writer queue")?;
        loop {
            if let Err(error) = PtyWriteState::ensure_open(&queue_guard) {
                drop(queue_guard);
                return Err(error);
            }
            if !self.state.reserve_write_bytes(&mut queue_guard, write_len)? {
                let observed_progress = queue_guard.progress_version;
                queue_guard = self.state.wait_for_queue_progress(queue_guard, observed_progress)?;
                continue;
            }
            match self.sender.try_send(PtyWriteRequest::Write(write)) {
                Ok(()) => {
                    drop(queue_guard);
                    return Ok(());
                }
                Err(TrySendError::Full(PtyWriteRequest::Write(returned))) => {
                    write = returned;
                    PtyWriteState::release_reserved_write_bytes(&mut queue_guard, write_len);
                    let observed_progress = queue_guard.progress_version;
                    queue_guard = self.state.wait_for_queue_progress(queue_guard, observed_progress)?;
                }
                Err(TrySendError::Disconnected(PtyWriteRequest::Write(_))) => {
                    PtyWriteState::release_reserved_write_bytes(&mut queue_guard, write_len);
                    drop(queue_guard);
                    return Err(self.state.stopped_report("reason=pty writer channel disconnected"));
                }
                Err(
                    TrySendError::Full(PtyWriteRequest::Shutdown)
                    | TrySendError::Disconnected(PtyWriteRequest::Shutdown),
                ) => {
                    drop(queue_guard);
                    return Err(report!("unexpected muxr pty writer enqueue send result"));
                }
            }
        }
    }

    fn shutdown(&self) -> rootcause::Result<()> {
        self.state.close()?;
        match self.sender.try_send(PtyWriteRequest::Shutdown) {
            Ok(()) | Err(TrySendError::Full(PtyWriteRequest::Shutdown)) => Ok(()),
            Err(TrySendError::Disconnected(PtyWriteRequest::Shutdown)) => {
                Err(self.state.stopped_report("reason=pty writer channel disconnected"))
            }
            Err(
                TrySendError::Full(PtyWriteRequest::Write(_)) | TrySendError::Disconnected(PtyWriteRequest::Write(_)),
            ) => Err(report!("unexpected muxr pty writer shutdown send result")),
        }
    }
}

// Client input used to lock and flush the PTY writer inline, so held-key latency paid for writer backpressure on the
// request path. Keep all shell-bound writes on one queue so input, paste, mouse, focus, and terminal replies preserve
// PTY order while the writer thread batches adjacent writes into one flush. The bounded queue and capped drain batch
// prevent a stalled PTY from growing memory without reintroducing normal-path per-key flush latency.
enum PtyWriteRequest {
    Write(PtyWrite),
    Shutdown,
}

struct PtyWrite {
    bytes: Vec<u8>,
    flush_context: &'static str,
    write_context: &'static str,
}

impl PtyWrite {
    const fn new(bytes: Vec<u8>, write_context: &'static str, flush_context: &'static str) -> Self {
        Self {
            bytes,
            flush_context,
            write_context,
        }
    }

    const fn len(&self) -> usize {
        self.bytes.len()
    }
}

// A full bounded queue can leave the reader waiting to enqueue a terminal reply while session shutdown waits to join
// that reader. The byte budget is tracked beside the send predicate so a stalled PTY cannot retain unbounded paste
// payloads while close/error/progress still wakes blocked enqueues without periodic polling.
struct PtyWriteState {
    byte_limit: usize,
    queue: Mutex<PtyWriteQueueState>,
    queue_progress: Condvar,
}

impl PtyWriteState {
    const fn new() -> Self {
        Self::with_byte_limit(PTY_WRITE_QUEUE_BYTE_LIMIT)
    }

    const fn with_byte_limit(byte_limit: usize) -> Self {
        Self {
            byte_limit,
            queue: Mutex::new(PtyWriteQueueState {
                closed: false,
                last_error: None,
                progress_version: 0,
                queued_bytes: 0,
            }),
            queue_progress: Condvar::new(),
        }
    }

    fn close(&self) -> rootcause::Result<()> {
        let mut queue = self::lock_mutex(&self.queue, "pty writer queue")?;
        queue.closed = true;
        drop(queue);
        self.notify_queue_progress();
        Ok(())
    }

    fn is_closed(&self) -> rootcause::Result<bool> {
        Ok(self::lock_mutex(&self.queue, "pty writer queue")?.closed)
    }

    fn ensure_open(queue: &PtyWriteQueueState) -> rootcause::Result<()> {
        if let Some(error) = queue.last_error.as_ref() {
            return Err(report!("muxr pty writer stopped").attach(format!("error={error}")));
        }
        if queue.closed {
            return Err(report!("muxr pty writer stopped").attach("reason=pty writer is closed"));
        }
        Ok(())
    }

    fn record_error(&self, error: &rootcause::Report) {
        match self.queue.lock() {
            Ok(mut queue) => {
                queue.closed = true;
                queue.last_error = Some(error.to_string());
            }
            Err(error) => {
                crate::session::tracing::pty::shutdown_failed("record_writer_error", error);
            }
        }
        self.notify_queue_progress();
    }

    fn reserve_write_bytes(&self, queue: &mut PtyWriteQueueState, write_len: usize) -> rootcause::Result<bool> {
        if write_len > self.byte_limit {
            return Err(report!("muxr pty write exceeded queue byte limit")
                .attach(format!("write_len={write_len}"))
                .attach(format!("byte_limit={}", self.byte_limit)));
        }
        let Some(remaining) = self.byte_limit.checked_sub(queue.queued_bytes) else {
            return Err(report!("muxr pty write queue byte accounting underflowed")
                .attach(format!("queued_bytes={}", queue.queued_bytes))
                .attach(format!("byte_limit={}", self.byte_limit)));
        };
        if write_len > remaining {
            return Ok(false);
        }
        queue.queued_bytes = queue
            .queued_bytes
            .checked_add(write_len)
            .ok_or_else(|| report!("muxr pty write queue byte accounting overflowed"))?;
        Ok(true)
    }

    const fn release_reserved_write_bytes(queue: &mut PtyWriteQueueState, write_len: usize) {
        queue.queued_bytes = queue.queued_bytes.saturating_sub(write_len);
        self::advance_queue_progress(queue);
    }

    fn release_queued_bytes(&self, bytes: usize) {
        match self.queue.lock() {
            Ok(mut queue) => Self::release_reserved_write_bytes(&mut queue, bytes),
            Err(error) => {
                crate::session::tracing::pty::shutdown_failed("release_writer_queue_bytes", error);
            }
        }
        self.queue_progress.notify_all();
    }

    fn stopped_report(&self, reason: &'static str) -> rootcause::Report {
        let mut report = report!("muxr pty writer stopped").attach(reason);
        if let Ok(queue) = self.queue.lock()
            && let Some(error) = queue.last_error.as_ref()
        {
            report = report.attach(format!("error={error}"));
        }
        report
    }

    fn wait_for_queue_progress<'a>(
        &self,
        guard: MutexGuard<'a, PtyWriteQueueState>,
        observed_progress: u64,
    ) -> rootcause::Result<MutexGuard<'a, PtyWriteQueueState>> {
        self.queue_progress
            .wait_while(guard, |queue| {
                !queue.closed && queue.last_error.is_none() && queue.progress_version == observed_progress
            })
            .map_err(|_| report!("poisoned muxr pty writer queue mutex"))
    }

    fn notify_queue_progress(&self) {
        match self.queue.lock() {
            Ok(mut queue) => {
                self::advance_queue_progress(&mut queue);
            }
            Err(error) => {
                crate::session::tracing::pty::shutdown_failed("record_writer_queue_progress", error);
            }
        }
        self.queue_progress.notify_all();
    }
}

struct PtyWriteQueueState {
    closed: bool,
    last_error: Option<String>,
    progress_version: u64,
    queued_bytes: usize,
}

const fn advance_queue_progress(queue: &mut PtyWriteQueueState) {
    queue.progress_version = queue.progress_version.wrapping_add(1);
}

#[derive(Clone)]
pub struct PtyHandle {
    child_killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    child_process_id: Option<u32>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    state: Arc<PtyState>,
    writer: PtyWriteQueue,
}

impl PtyHandle {
    pub fn attach_sink(&self, sender: mpsc::SyncSender<PtyEvent>) -> rootcause::Result<PtySinkGuard> {
        self.state.attach_sink(sender)
    }

    pub fn has_exited(&self) -> bool {
        self.state.exited.load(Ordering::Acquire)
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
        self::queue_pty_bytes(
            &self.writer,
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
        let framed = crate::terminal::paste_input_bytes(bytes, bracketed_paste_enabled);
        self::queue_pty_write(
            &self.writer,
            &framed,
            "failed to write client paste to muxr shell pty",
            "failed to flush muxr shell pty paste",
        )?;
        Ok(scrolled_to_bottom)
    }

    pub fn write_mouse_event(
        &self,
        event: ClientMouseEvent,
        region: &PaneRegionSnapshot,
        protocol: TerminalMouseProtocol,
    ) -> rootcause::Result<Option<bool>> {
        let Some(bytes) = crate::pane::mouse::encode_pty_mouse_event(event, region, protocol)? else {
            return Ok(None);
        };
        // Scrollback follows only events that reach the PTY, so filtered motion does not hide history.
        let scrolled_to_bottom = lock_mutex(&self.state.terminal, "pty terminal")?.scroll_to_bottom();
        self::queue_pty_bytes(
            &self.writer,
            &bytes,
            "failed to write client mouse event to muxr shell pty",
            "failed to flush muxr shell pty mouse event",
        )?;
        Ok(Some(scrolled_to_bottom))
    }

    pub fn write_faux_scroll_input(
        &self,
        direction: PaneScrollDirection,
        cursor_key_mode: TerminalCursorKeyMode,
    ) -> rootcause::Result<bool> {
        self.write_input(&crate::pane::scroll::faux_scroll_input_bytes(
            direction,
            cursor_key_mode,
        ))
    }

    pub fn write_focus_event(&self, event: TerminalFocusEvent) -> rootcause::Result<()> {
        let focus_reporting = self.application_mode()?.focus_reporting;
        self::write_pty_focus_event(&self.writer, focus_reporting, event)
    }

    pub fn mouse_mode(&self) -> rootcause::Result<PaneMouseMode> {
        Ok(self.application_mode()?.pane_mouse_mode())
    }

    pub fn application_mode(&self) -> rootcause::Result<TerminalApplicationMode> {
        Ok(lock_mutex(&self.state.terminal, "pty terminal")?.application_mode())
    }

    pub fn scroll(&self, direction: PaneScrollDirection) -> rootcause::Result<bool> {
        Ok(lock_mutex(&self.state.terminal, "pty terminal")?.scroll(direction))
    }

    pub fn scroll_one_line(&self, direction: PaneScrollDirection) -> rootcause::Result<bool> {
        Ok(lock_mutex(&self.state.terminal, "pty terminal")?.scroll_one_line(direction))
    }

    pub fn visible_top_row(&self) -> rootcause::Result<u64> {
        lock_mutex(&self.state.terminal, "pty terminal")?.visible_top_row()
    }

    pub fn exit_status(&self) -> rootcause::Result<Option<PtyExitStatus>> {
        Ok(lock_mutex(&self.state.exit_status, "pty exit status")?.clone())
    }

    pub const fn process_id(&self) -> Option<u32> {
        self.child_process_id
    }

    pub fn fg_process_group(&self) -> rootcause::Result<Option<u32>> {
        Ok(lock_mutex(&self.master, "pty master")?
            .process_group_leader()
            .and_then(|process_group| u32::try_from(process_group).ok())
            .filter(|process_group| *process_group != 0))
    }

    pub fn terminal_title(&self) -> rootcause::Result<Option<String>> {
        Ok(lock_mutex(&self.state.terminal, "pty terminal")?.title())
    }

    pub fn take_title_changes(&self) -> rootcause::Result<Vec<Option<String>>> {
        self.state.take_title_changes()
    }

    pub fn take_screen_dirty(&self) -> bool {
        self.state.take_screen_dirty()
    }

    pub fn render_snapshot(&self) -> rootcause::Result<TerminalSnapshot> {
        lock_mutex(&self.state.terminal, "pty terminal")?.snapshot()
    }

    pub fn write_scrollback_dump(&self, style: ScrollbackDumpStyle, writer: &mut impl Write) -> rootcause::Result<()> {
        let dump = lock_mutex(&self.state.terminal, "pty terminal")?
            .scrollback_dump(style)
            .context("failed to build muxr scrollback dump")?;
        Ok(writer
            .write_all(&dump)
            .context("failed to write muxr scrollback dump")?)
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
    pane_exit_notify: Arc<tokio::sync::Notify>,
    screen_dirty: AtomicBool,
    terminal: Mutex<TerminalState>,
    title_changes: Mutex<Vec<Option<String>>>,
}

impl PtyState {
    fn with_history(
        size: &TerminalSize,
        history_path: &Path,
        scrollback: ScrollbackConfig,
        pane_exit_notify: Arc<tokio::sync::Notify>,
    ) -> rootcause::Result<Self> {
        let (history, replay) = PaneHistory::open(history_path)?;
        let mut terminal = TerminalState::with_scrollback(size, scrollback);
        let _ = terminal.process(&replay);
        // History replay rebuilds visible cells only; metadata and app-owned modes must come from live PTY output
        // after spawn.
        terminal.clear_title_metadata();
        terminal.clear_replayed_application_state();

        Ok(Self {
            active_sink: Mutex::new(None),
            exited: AtomicBool::new(false),
            exit_status: Mutex::new(None),
            history: Mutex::new(Some(history)),
            pane_exit_notify,
            screen_dirty: AtomicBool::new(false),
            terminal: Mutex::new(terminal),
            title_changes: Mutex::new(Vec::new()),
        })
    }

    fn attach_sink(self: &Arc<Self>, sender: mpsc::SyncSender<PtyEvent>) -> rootcause::Result<PtySinkGuard> {
        let output_current = Arc::new(AtomicBool::new(true));
        lock_mutex(&self.title_changes, "pty title changes")?.clear();
        // Attach sends a fresh baseline; discard dirty state accumulated before the client could observe output events.
        self.screen_dirty.store(false, Ordering::Release);
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
        let terminal_replies = {
            let mut terminal = lock_mutex(&self.terminal, "pty terminal")?;
            let process_outcome = terminal.process(bytes);
            if process_outcome.screen_dirty() {
                // Output events are coalesced, so the visible-screen dirty bit must be sticky until the server consumes
                // it.
                self.screen_dirty.store(true, Ordering::Release);
            }
            let terminal_replies = process_outcome.into_replies();
            let title_changes = terminal.take_title_changes();
            drop(terminal);
            // Title changes are queued separately from coalesced output events so cmd->cwd title transitions are
            // not collapsed before the server can emit matching tab bar updates.
            let active_sink = lock_mutex(&self.active_sink, "pty active sink")?;
            if !title_changes.is_empty() && active_sink.is_some() {
                lock_mutex(&self.title_changes, "pty title changes")?.extend(title_changes);
            }
            drop(active_sink);
            terminal_replies
        };

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

    fn take_title_changes(&self) -> rootcause::Result<Vec<Option<String>>> {
        let mut title_changes = lock_mutex(&self.title_changes, "pty title changes")?;
        Ok(std::mem::take(&mut *title_changes))
    }

    fn take_screen_dirty(&self) -> bool {
        self.screen_dirty.swap(false, Ordering::AcqRel)
    }

    fn mark_exited(&self, exit_status: PtyExitStatus) -> rootcause::Result<()> {
        let mut stored_exit_status = lock_mutex(&self.exit_status, "pty exit status")?;
        if stored_exit_status.is_none() {
            *stored_exit_status = Some(exit_status);
        }
        drop(stored_exit_status);

        self.exited.store(true, Ordering::Release);
        // Detached sessions have no active PTY sink; notify the server loop so it can reap sticky exit state.
        self.pane_exit_notify.notify_one();

        let mut active_sink = lock_mutex(&self.active_sink, "pty active sink")?;
        if let Some(sink) = active_sink.as_ref() {
            match sink.sender.try_send(PtyEvent::Exited) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    crate::session::tracing::pty::exit_wakeup_not_queued("channel_full");
                }
                Err(TrySendError::Disconnected(_)) => {
                    crate::session::tracing::pty::exit_wakeup_not_queued("channel_disconnected");
                    sink.output_current.store(false, Ordering::Release);
                    *active_sink = None;
                }
            }
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

fn resolved_cwd(cwd: &str) -> rootcause::Result<PathBuf> {
    // Pane cwd comes from restored layout or shell-title metadata; falling back to the server cwd opens panes
    // elsewhere.
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return Err(report!("invalid muxr pane cwd").attach("reason=cwd must not be empty"));
    }

    let path = if cwd == "~" {
        env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| report!("invalid muxr pane cwd").attach("reason=HOME is not set"))?
    } else if let Some(rest) = cwd.strip_prefix("~/").filter(|rest| !rest.is_empty()) {
        env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| report!("invalid muxr pane cwd").attach("reason=HOME is not set"))?
            .join(rest)
    } else {
        PathBuf::from(cwd)
    };

    if !path.is_dir() {
        return Err(report!("invalid muxr pane cwd")
            .attach("reason=path is not a directory")
            .attach(format!("cwd={cwd}"))
            .attach(format!("path={}", path.display())));
    }

    Ok(path)
}

fn shell_quote(raw: &str) -> String {
    if raw.is_empty() {
        return "''".to_owned();
    }

    if raw.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'/' | b'.' | b'_' | b'-' | b':' | b'+' | b'=' | b',' | b'@' | b'%'
            )
    }) {
        return raw.to_owned();
    }

    format!("'{}'", raw.replace('\'', "'\\''"))
}

fn lock_mutex<'a, T>(mutex: &'a Mutex<T>, name: &str) -> rootcause::Result<MutexGuard<'a, T>> {
    mutex.lock().map_err(|_| report!("poisoned muxr {name} mutex"))
}

fn queue_pty_bytes(
    writer: &PtyWriteQueue,
    bytes: &[u8],
    write_context: &'static str,
    flush_context: &'static str,
) -> rootcause::Result<()> {
    self::queue_pty_write(writer, bytes, write_context, flush_context)
}

fn queue_pty_write(
    writer: &PtyWriteQueue,
    bytes: &[u8],
    write_context: &'static str,
    flush_context: &'static str,
) -> rootcause::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }

    for chunk in bytes.chunks(PTY_WRITE_MAX_MESSAGE_BYTES) {
        writer.enqueue(PtyWrite::new(chunk.to_vec(), write_context, flush_context))?;
    }
    Ok(())
}

fn write_pty_focus_event(
    writer: &PtyWriteQueue,
    focus_reporting: TerminalFocusReporting,
    event: TerminalFocusEvent,
) -> rootcause::Result<()> {
    match focus_reporting {
        TerminalFocusReporting::Disabled => Ok(()),
        TerminalFocusReporting::Enabled => {
            self::queue_pty_bytes(
                writer,
                event.bytes(),
                "failed to write muxr terminal focus event to shell pty",
                "failed to flush muxr terminal focus event",
            )?;
            Ok(())
        }
    }
}

fn write_terminal_replies(writer: &PtyWriteQueue, replies: &[Vec<u8>]) -> rootcause::Result<()> {
    if replies.is_empty() {
        return Ok(());
    }

    let bytes_len = replies
        .iter()
        .try_fold(0_usize, |sum, reply| sum.checked_add(reply.len()))
        .ok_or_else(|| report!("muxr terminal reply bytes overflowed"))?;
    let mut bytes = Vec::with_capacity(bytes_len);
    for reply in replies {
        bytes.extend_from_slice(reply);
    }
    self::queue_pty_write(
        writer,
        &bytes,
        "failed to write muxr terminal reply to shell pty",
        "failed to flush muxr terminal reply to shell pty",
    )?;
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

fn spawn_writer_thread(mut writer: Box<dyn Write + Send>) -> (PtyWriteQueue, thread::JoinHandle<()>) {
    let (sender, receiver) = mpsc::sync_channel(PTY_WRITE_QUEUE_LIMIT);
    let state = Arc::new(PtyWriteState::new());
    let queue = PtyWriteQueue {
        sender,
        state: Arc::clone(&state),
    };
    // Raw OS threads do not inherit thread-local tracing state, so carry both the dispatcher and span explicitly.
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let writer_handle = thread::spawn(move || {
        tracing::dispatcher::with_default(&dispatch, || {
            let _guard = span.enter();
            self::run_writer_loop(&mut *writer, &receiver, state.as_ref());
        });
    });
    (queue, writer_handle)
}

fn spawn_reader_thread(
    mut reader: Box<dyn Read + Send>,
    state: Arc<PtyState>,
    writer: PtyWriteQueue,
) -> thread::JoinHandle<()> {
    // Raw OS threads do not inherit thread-local tracing state, so carry both the dispatcher and span explicitly.
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    thread::spawn(move || {
        tracing::dispatcher::with_default(&dispatch, || {
            let _guard = span.enter();
            self::run_reader_loop(&mut *reader, state.as_ref(), &writer);
        });
    })
}

fn spawn_child_wait_thread(mut child: Box<dyn Child + Send + Sync>, state: Arc<PtyState>) -> thread::JoinHandle<()> {
    // Raw OS threads do not inherit thread-local tracing state, so carry both the dispatcher and span explicitly.
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    thread::spawn(move || {
        tracing::dispatcher::with_default(&dispatch, || {
            let _guard = span.enter();
            match child.wait() {
                Ok(exit_status) => {
                    let _ = state
                        .mark_exited(PtyExitStatus::from(&exit_status))
                        .inspect_err(|error| {
                            crate::session::tracing::pty::shutdown_failed("mark_exited", error);
                        });
                }
                Err(error) => {
                    crate::session::tracing::pty::shutdown_failed("wait_child", &error);
                }
            }
        });
    })
}

fn run_reader_loop(reader: &mut dyn Read, state: &PtyState, writer: &PtyWriteQueue) {
    let mut buffer = [0; READ_BUFFER_SIZE];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => {
                // PTY EOF only means the slave side closed; the child may still be running
                // after redirecting stdio, so only the child wait thread is allowed to mark exit.
                break;
            }
            Err(_) => {
                // Read errors stop only the reader loop; the child wait thread still owns exit detection, and a later
                // input/write path will surface broken PTY state if it remains user-visible.
                break;
            }
            Ok(bytes_read) => {
                let Some(bytes) = buffer.get(..bytes_read) else {
                    break;
                };
                let terminal_replies = match state.append_output(bytes) {
                    Ok(terminal_replies) => terminal_replies,
                    Err(error) => {
                        crate::session::tracing::pty::reader_stopped_after_error("append_output", &error);
                        break;
                    }
                };
                if self::write_terminal_replies(writer, &terminal_replies)
                    .inspect_err(|error| {
                        crate::session::tracing::pty::reader_stopped_after_error("write_terminal_replies", error);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}

fn run_writer_loop(writer: &mut dyn Write, receiver: &mpsc::Receiver<PtyWriteRequest>, state: &PtyWriteState) {
    let mut batch = Vec::new();
    loop {
        let request = match state.is_closed() {
            Ok(true) => match receiver.try_recv() {
                Ok(request) => request,
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            },
            Ok(false) => match receiver.recv() {
                Ok(request) => request,
                Err(_) => break,
            },
            Err(error) => {
                crate::session::tracing::pty::shutdown_failed("read_writer_state", &error);
                break;
            }
        };
        match request {
            PtyWriteRequest::Write(write) => {
                let mut batch_bytes = write.bytes.len();
                batch.push(write);
                let shutdown = self::drain_pending_writes(receiver, &mut batch, &mut batch_bytes);
                let write_result = self::write_pty_batch(writer, &batch);
                state.release_queued_bytes(batch_bytes);
                if let Err(error) = write_result {
                    state.record_error(&error);
                    crate::session::tracing::pty::writer_stopped_after_error("write_batch", &error);
                    break;
                }
                batch.clear();
                if shutdown {
                    break;
                }
            }
            PtyWriteRequest::Shutdown => break,
        }
    }
}

fn drain_pending_writes(
    receiver: &mpsc::Receiver<PtyWriteRequest>,
    batch: &mut Vec<PtyWrite>,
    batch_bytes: &mut usize,
) -> bool {
    loop {
        if batch.len() >= PTY_WRITE_BATCH_MAX_MESSAGES || *batch_bytes >= PTY_WRITE_BATCH_MAX_BYTES {
            return false;
        }
        match receiver.try_recv() {
            Ok(PtyWriteRequest::Write(write)) => {
                *batch_bytes = batch_bytes.saturating_add(write.bytes.len());
                batch.push(write);
            }
            Ok(PtyWriteRequest::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => return true,
            Err(mpsc::TryRecvError::Empty) => return false,
        }
    }
}

fn write_pty_batch(writer: &mut dyn Write, batch: &[PtyWrite]) -> rootcause::Result<()> {
    let Some(first_write) = batch.first() else {
        return Ok(());
    };
    for write in batch {
        writer.write_all(write.bytes.as_slice()).context(write.write_context)?;
    }
    let flush_context = if batch.len() == 1 {
        first_write.flush_context
    } else {
        "failed to flush muxr shell pty write batch"
    };
    writer.flush().context(flush_context)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use muxr_config::MuxrConfig;
    use muxr_core::SessionName;

    use super::*;

    fn pty_state(size: &TerminalSize) -> PtyState {
        PtyState {
            active_sink: Mutex::new(None),
            exited: AtomicBool::new(false),
            exit_status: Mutex::new(None),
            history: Mutex::new(None),
            pane_exit_notify: Arc::new(tokio::sync::Notify::new()),
            screen_dirty: AtomicBool::new(false),
            terminal: Mutex::new(TerminalState::with_scrollback(size, MuxrConfig::default().scrollback)),
            title_changes: Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn test_shell_cmd_new_when_program_is_empty_returns_error() {
        assert2::assert!(ShellCmd::new("").is_err());
    }

    #[test]
    fn test_shell_cmd_cmd_builder_when_cwd_exists_sets_cwd() -> rootcause::Result<()> {
        let cwd = tempfile::tempdir()?;
        let cmd = ShellCmd::new("/bin/sh")?.cmd_builder(cwd.path().to_string_lossy().as_ref())?;

        pretty_assertions::assert_eq!(cmd.get_cwd().map(PathBuf::from), Some(cwd.path().to_path_buf()),);
        Ok(())
    }

    #[test]
    fn test_shell_cmd_cmd_builder_when_cwd_is_missing_returns_error() -> rootcause::Result<()> {
        let cwd = tempfile::tempdir()?;
        let missing = cwd.path().join("missing");

        assert2::assert!(
            ShellCmd::new("/bin/sh")?
                .cmd_builder(missing.to_string_lossy().as_ref())
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_spawn_reader_thread_when_pty_reaches_eof_does_not_mark_child_exited() -> rootcause::Result<()> {
        let state = Arc::new(pty_state(&terminal_size()?));
        let (writer, writer_handle) = self::spawn_writer_thread(self::sink_pty_writer());
        let reader_handle = spawn_reader_thread(
            Box::new(std::io::Cursor::new(Vec::new())),
            Arc::clone(&state),
            writer.clone(),
        );

        reader_handle
            .join()
            .map_err(|_| report!("muxr pty reader test thread panicked"))?;
        writer.shutdown()?;
        writer_handle
            .join()
            .map_err(|_| report!("muxr pty writer test thread panicked"))?;

        assert2::assert!(!state.exited.load(Ordering::Acquire));
        assert2::assert!(lock_mutex(&state.exit_status, "pty exit status")?.is_none());
        Ok(())
    }

    #[test]
    fn test_pty_session_when_child_exits_sends_exit_event() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let history_path = tempdir.path().join("1").join("output.raw");
        std::fs::create_dir_all(
            history_path
                .parent()
                .ok_or_else(|| report!("expected history parent"))?,
        )?;
        let session = PtySession::spawn(
            &ShellCmd::with_args("/bin/sh", ["-c", "sleep 0.1; exit 7"])?,
            "/tmp",
            &terminal_size()?,
            &history_path,
            MuxrConfig::default().scrollback,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        let handle = session.handle();
        let (sender, receiver) = mpsc::sync_channel(1);
        let _guard = handle.attach_sink(sender)?;

        assert2::assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(2)),
            Ok(PtyEvent::Exited)
        ));
        pretty_assertions::assert_eq!(
            handle.exit_status()?,
            Some(PtyExitStatus {
                code: 7,
                signal: None,
                success: false,
            })
        );
        drop(session);
        Ok(())
    }

    #[test]
    fn test_pty_state_mark_exited_when_exit_wakeup_channel_is_full_warns_and_keeps_sticky_state()
    -> rootcause::Result<()> {
        let state = pty_state(&terminal_size()?);
        let output_current = Arc::new(AtomicBool::new(true));
        let (sender, _receiver) = mpsc::sync_channel(0);
        *lock_mutex(&state.active_sink, "pty active sink")? = Some(ActivePtySink {
            output_current: Arc::clone(&output_current),
            sender,
        });
        let session = SessionName::default();

        let log = crate::session::tracing::collect_test_log(&session, || {
            state.mark_exited(PtyExitStatus {
                code: 7,
                signal: None,
                success: false,
            })
        })?;

        assert2::assert!(state.exited.load(Ordering::Acquire));
        assert2::assert!(output_current.load(Ordering::Acquire));
        assert2::assert!(lock_mutex(&state.active_sink, "pty active sink")?.is_some());
        pretty_assertions::assert_eq!(
            *lock_mutex(&state.exit_status, "pty exit status")?,
            Some(PtyExitStatus {
                code: 7,
                signal: None,
                success: false,
            })
        );
        assert2::assert!(log.contains("kind=\"pty_exit_wakeup_not_queued\""));
        assert2::assert!(log.contains("reason=\"channel_full\""));
        Ok(())
    }

    #[test]
    fn test_attach_sink_when_output_arrives_after_attach_delivers_live_event() -> rootcause::Result<()> {
        let state = Arc::new(pty_state(&terminal_size()?));
        pretty_assertions::assert_eq!(state.append_output(b"before")?, Vec::<Vec<u8>>::new());
        let (sender, receiver) = mpsc::sync_channel(1);

        let _guard = state.attach_sink(sender)?;
        pretty_assertions::assert_eq!(state.append_output(b"after")?, Vec::<Vec<u8>>::new());

        assert2::assert!(matches!(receiver.recv(), Ok(PtyEvent::OutputReady)));
        Ok(())
    }

    #[test]
    fn test_attach_sink_when_output_arrived_before_attach_clears_screen_dirty() -> rootcause::Result<()> {
        let state = Arc::new(pty_state(&terminal_size()?));
        pretty_assertions::assert_eq!(state.append_output(b"before")?, Vec::<Vec<u8>>::new());
        assert2::assert!(state.take_screen_dirty());
        pretty_assertions::assert_eq!(state.append_output(b"before again")?, Vec::<Vec<u8>>::new());
        let (sender, _receiver) = mpsc::sync_channel(1);

        let _guard = state.attach_sink(sender)?;

        assert2::assert!(!state.take_screen_dirty());
        Ok(())
    }

    #[test]
    fn test_append_output_when_title_only_changes_does_not_mark_screen_dirty() -> rootcause::Result<()> {
        let state = Arc::new(pty_state(&terminal_size()?));
        let (sender, receiver) = mpsc::sync_channel(1);
        let _guard = state.attach_sink(sender)?;

        pretty_assertions::assert_eq!(state.append_output(b"\x1b]2;~\x07")?, Vec::<Vec<u8>>::new());

        assert2::assert!(!state.take_screen_dirty());
        pretty_assertions::assert_eq!(state.take_title_changes()?, vec![Some("~".to_owned())]);
        assert2::assert!(matches!(receiver.recv(), Ok(PtyEvent::OutputReady)));
        Ok(())
    }

    #[test]
    fn test_append_output_when_visible_output_arrives_marks_screen_dirty_until_taken() -> rootcause::Result<()> {
        let state = pty_state(&terminal_size()?);

        pretty_assertions::assert_eq!(state.append_output(b"visible")?, Vec::<Vec<u8>>::new());

        assert2::assert!(state.take_screen_dirty());
        assert2::assert!(!state.take_screen_dirty());
        Ok(())
    }

    #[test]
    fn test_with_history_when_history_contains_title_does_not_restore_live_title() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let history_path = tempdir.path().join("1").join("output.raw");
        std::fs::create_dir_all(
            history_path
                .parent()
                .ok_or_else(|| report!("expected history parent"))?,
        )?;
        std::fs::write(&history_path, b"\x1b]2;~\x07history")?;

        let state = PtyState::with_history(
            &terminal_size()?,
            &history_path,
            MuxrConfig::default().scrollback,
            Arc::new(tokio::sync::Notify::new()),
        )?;

        pretty_assertions::assert_eq!(lock_mutex(&state.terminal, "pty terminal")?.title(), None);
        pretty_assertions::assert_eq!(state.take_title_changes()?, Vec::<Option<String>>::new());
        Ok(())
    }

    #[test]
    fn test_with_history_when_history_contains_focus_reporting_does_not_restore_live_mode() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let history_path = tempdir.path().join("1").join("output.raw");
        std::fs::create_dir_all(
            history_path
                .parent()
                .ok_or_else(|| report!("expected history parent"))?,
        )?;
        std::fs::write(&history_path, b"\x1b[?1004h")?;

        let state = PtyState::with_history(
            &terminal_size()?,
            &history_path,
            MuxrConfig::default().scrollback,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        let mode = lock_mutex(&state.terminal, "pty terminal")?.application_mode();

        pretty_assertions::assert_eq!(mode.focus_reporting, TerminalFocusReporting::Disabled);
        Ok(())
    }

    #[test]
    fn test_append_output_when_sink_is_full_coalesces_without_blocking() -> rootcause::Result<()> {
        let state = pty_state(&terminal_size()?);
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
        assert2::assert!(state.take_screen_dirty());
        assert2::assert!(!state.take_screen_dirty());
        assert2::assert!(matches!(receiver.recv(), Ok(PtyEvent::OutputReady)));
        Ok(())
    }

    #[test]
    fn test_append_output_when_sink_is_full_and_title_changes_preserves_title_changes() -> rootcause::Result<()> {
        let state = pty_state(&terminal_size()?);
        let (sender, receiver) = mpsc::sync_channel(1);
        let output_current = Arc::new(AtomicBool::new(true));
        *lock_mutex(&state.active_sink, "pty active sink")? = Some(ActivePtySink {
            output_current: Arc::clone(&output_current),
            sender,
        });

        pretty_assertions::assert_eq!(state.append_output(b"first")?, Vec::<Vec<u8>>::new());
        pretty_assertions::assert_eq!(
            state.append_output(b"\x1b]2;cargo test\x07\x1b]2;~\x07")?,
            Vec::<Vec<u8>>::new()
        );

        pretty_assertions::assert_eq!(
            state.take_title_changes()?,
            vec![Some("cargo test".to_owned()), Some("~".to_owned())],
        );
        assert2::assert!(matches!(receiver.recv(), Ok(PtyEvent::OutputReady)));
        Ok(())
    }

    #[test]
    fn test_append_output_when_terminal_reply_is_generated_writes_reply_to_pty() -> rootcause::Result<()> {
        let state = pty_state(&terminal_size()?);
        let (queue, receiver) = self::queued_pty_writer();
        let written = Arc::new(Mutex::new(Vec::new()));

        let replies = state.append_output(b"\x1b[6n")?;
        self::write_terminal_replies(&queue, &replies)?;
        self::drain_queued_writes(&queue, &receiver, self::capturing_pty_writer(Arc::clone(&written)))?;

        pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, b"\x1b[1;1R".to_vec());
        Ok(())
    }

    #[test]
    fn test_run_writer_loop_when_multiple_writes_are_pending_batches_in_order() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer();
        let written = Arc::new(Mutex::new(Vec::new()));
        let flushes = Arc::new(Mutex::new(0_usize));

        queue.enqueue(PtyWrite::new(
            b"one".to_vec(),
            "failed to write test one",
            "failed to flush test one",
        ))?;
        queue.enqueue(PtyWrite::new(
            b"two".to_vec(),
            "failed to write test two",
            "failed to flush test two",
        ))?;
        self::drain_queued_writes(
            &queue,
            &receiver,
            Box::new(FlushCountingWriter {
                flushes: Arc::clone(&flushes),
                written: Arc::clone(&written),
            }),
        )?;

        pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, b"onetwo".to_vec());
        pretty_assertions::assert_eq!(self::captured_flushes(flushes.as_ref())?, 1);
        Ok(())
    }

    #[test]
    fn test_pty_write_queue_when_limit_reached_applies_backpressure() -> rootcause::Result<()> {
        let (queue, _receiver) = self::queued_pty_writer_with_limit(1);
        queue.enqueue(PtyWrite::new(
            b"one".to_vec(),
            "failed to write first bounded test payload",
            "failed to flush first bounded test payload",
        ))?;

        assert2::assert!(matches!(
            queue.sender.try_send(PtyWriteRequest::Write(PtyWrite::new(
                b"two".to_vec(),
                "failed to write second bounded test payload",
                "failed to flush second bounded test payload",
            ))),
            Err(mpsc::TrySendError::Full(PtyWriteRequest::Write(_)))
        ));
        Ok(())
    }

    #[test]
    fn test_pty_write_queue_when_byte_limit_reached_applies_backpressure_until_written() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer_with_limits(4, 3);
        let written = Arc::new(Mutex::new(Vec::new()));
        queue.enqueue(PtyWrite::new(
            b"abc".to_vec(),
            "failed to write first byte-budget test payload",
            "failed to flush first byte-budget test payload",
        ))?;
        let (waiting_started_sender, waiting_started_receiver) = mpsc::channel();
        let (waiting_done_sender, waiting_done_receiver) = mpsc::channel();
        let waiting_queue = queue.clone();
        let waiting_handle = thread::spawn(move || {
            let _started = waiting_started_sender.send(());
            let result = waiting_queue
                .enqueue(PtyWrite::new(
                    b"d".to_vec(),
                    "failed to write blocked byte-budget test payload",
                    "failed to flush blocked byte-budget test payload",
                ))
                .map_err(|error| error.to_string());
            let _sent = waiting_done_sender.send(result);
        });
        waiting_started_receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|error| {
                report!("muxr byte-budget waiting enqueue test thread did not start").attach(format!("error={error}"))
            })?;
        assert2::assert!(waiting_done_receiver.recv_timeout(Duration::from_millis(20)).is_err());

        let writer_queue = queue.clone();
        let writer_handle = thread::spawn({
            let written = Arc::clone(&written);
            move || {
                self::run_writer_loop(
                    &mut *self::capturing_pty_writer(written),
                    &receiver,
                    writer_queue.state.as_ref(),
                );
            }
        });
        waiting_done_receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|error| {
                report!("muxr byte-budget waiting enqueue did not unblock after writer progress")
                    .attach(format!("error={error}"))
            })?
            .map_err(|error| report!("muxr byte-budget waiting enqueue failed").attach(error))?;
        waiting_handle
            .join()
            .map_err(|_| report!("muxr byte-budget waiting enqueue test thread panicked"))?;
        queue.shutdown()?;
        writer_handle
            .join()
            .map_err(|_| report!("muxr byte-budget writer test thread panicked"))?;

        pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, b"abcd".to_vec());
        Ok(())
    }

    #[test]
    fn test_pty_write_queue_when_shutdown_races_full_queue_unblocks_waiting_enqueue() -> rootcause::Result<()> {
        let (queue, _receiver) = self::queued_pty_writer_with_limit(1);
        queue.enqueue(PtyWrite::new(
            b"queued".to_vec(),
            "failed to write queued shutdown-race test payload",
            "failed to flush queued shutdown-race test payload",
        ))?;
        let (waiting_started_sender, waiting_started_receiver) = mpsc::channel();
        let (waiting_done_sender, waiting_done_receiver) = mpsc::channel();
        let waiting_queue = queue.clone();
        let waiting_handle = thread::spawn(move || {
            let _started = waiting_started_sender.send(());
            let result = waiting_queue
                .enqueue(PtyWrite::new(
                    b"blocked".to_vec(),
                    "failed to write blocked shutdown-race test payload",
                    "failed to flush blocked shutdown-race test payload",
                ))
                .map_err(|error| error.to_string());
            let _sent = waiting_done_sender.send(result);
        });
        waiting_started_receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|error| {
                report!("muxr waiting enqueue test thread did not start").attach(format!("error={error}"))
            })?;
        thread::sleep(Duration::from_millis(10));

        let (shutdown_done_sender, shutdown_done_receiver) = mpsc::channel();
        let shutdown_queue = queue;
        let shutdown_handle = thread::spawn(move || {
            let result = shutdown_queue.shutdown().map_err(|error| error.to_string());
            let _sent = shutdown_done_sender.send(result);
        });

        shutdown_done_receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|error| {
                report!("muxr writer shutdown blocked behind waiting enqueue").attach(format!("error={error}"))
            })?
            .map_err(|error| report!("muxr writer shutdown failed").attach(error))?;
        let waiting_result = waiting_done_receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|error| {
                report!("muxr waiting enqueue did not unblock after shutdown").attach(format!("error={error}"))
            })?;
        waiting_handle
            .join()
            .map_err(|_| report!("muxr waiting enqueue test thread panicked"))?;
        shutdown_handle
            .join()
            .map_err(|_| report!("muxr writer shutdown test thread panicked"))?;
        let error = waiting_result
            .err()
            .ok_or_else(|| report!("expected waiting muxr pty write enqueue to fail after shutdown"))?;

        assert2::assert!(error.contains("pty writer is closed"));
        Ok(())
    }

    #[test]
    fn test_queue_pty_write_when_payload_exceeds_message_limit_chunks_in_order() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer_with_limits(4, PTY_WRITE_MAX_MESSAGE_BYTES.saturating_add(1));
        let written = Arc::new(Mutex::new(Vec::new()));
        let mut payload = vec![b'a'; PTY_WRITE_MAX_MESSAGE_BYTES];
        payload.push(b'b');

        self::queue_pty_write(
            &queue,
            payload.as_slice(),
            "failed to write chunked test payload",
            "failed to flush chunked test payload",
        )?;
        self::drain_queued_writes(&queue, &receiver, self::capturing_pty_writer(Arc::clone(&written)))?;

        pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, payload);
        Ok(())
    }

    #[test]
    fn test_run_writer_loop_when_closed_with_full_queue_drains_accepted_write() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer_with_limit(1);
        let written = Arc::new(Mutex::new(Vec::new()));
        queue.enqueue(PtyWrite::new(
            b"accepted".to_vec(),
            "failed to write accepted test payload",
            "failed to flush accepted test payload",
        ))?;

        queue.shutdown()?;
        self::run_writer_loop(
            &mut *self::capturing_pty_writer(Arc::clone(&written)),
            &receiver,
            queue.state.as_ref(),
        );

        pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, b"accepted".to_vec());
        Ok(())
    }

    #[test]
    fn test_run_writer_loop_when_message_batch_limit_is_reached_flushes_in_chunks() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer();
        let written = Arc::new(Mutex::new(Vec::new()));
        let flushes = Arc::new(Mutex::new(0_usize));

        for _ in 0..=PTY_WRITE_BATCH_MAX_MESSAGES {
            queue.enqueue(PtyWrite::new(
                b"x".to_vec(),
                "failed to write batch-limit test payload",
                "failed to flush batch-limit test payload",
            ))?;
        }
        self::drain_queued_writes(
            &queue,
            &receiver,
            Box::new(FlushCountingWriter {
                flushes: Arc::clone(&flushes),
                written: Arc::clone(&written),
            }),
        )?;

        pretty_assertions::assert_eq!(
            self::captured_pty_bytes(written.as_ref())?,
            vec![b'x'; PTY_WRITE_BATCH_MAX_MESSAGES + 1]
        );
        pretty_assertions::assert_eq!(self::captured_flushes(flushes.as_ref())?, 2);
        Ok(())
    }

    #[test]
    fn test_run_writer_loop_when_byte_batch_limit_is_reached_flushes_in_chunks() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer();
        let written = Arc::new(Mutex::new(Vec::new()));
        let flushes = Arc::new(Mutex::new(0_usize));
        let chunk_len = PTY_WRITE_BATCH_MAX_BYTES / 2;

        for byte in *b"ab" {
            queue.enqueue(PtyWrite::new(
                vec![byte; chunk_len],
                "failed to write byte-limit test payload",
                "failed to flush byte-limit test payload",
            ))?;
        }
        queue.enqueue(PtyWrite::new(
            b"c".to_vec(),
            "failed to write trailing byte-limit test payload",
            "failed to flush trailing byte-limit test payload",
        ))?;
        self::drain_queued_writes(
            &queue,
            &receiver,
            Box::new(FlushCountingWriter {
                flushes: Arc::clone(&flushes),
                written: Arc::clone(&written),
            }),
        )?;

        let written = self::captured_pty_bytes(written.as_ref())?;
        pretty_assertions::assert_eq!(written.len(), PTY_WRITE_BATCH_MAX_BYTES + 1);
        pretty_assertions::assert_eq!(written.first(), Some(&b'a'));
        pretty_assertions::assert_eq!(written.last(), Some(&b'c'));
        pretty_assertions::assert_eq!(self::captured_flushes(flushes.as_ref())?, 2);
        Ok(())
    }

    #[test]
    fn test_run_writer_loop_when_write_fails_stores_error_for_later_enqueue() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer();
        queue.enqueue(PtyWrite::new(
            b"first".to_vec(),
            "failed to write first test payload",
            "failed to flush first test payload",
        ))?;

        self::run_writer_loop(&mut *self::failing_pty_writer(), &receiver, queue.state.as_ref());
        let error = queue
            .enqueue(PtyWrite::new(
                b"second".to_vec(),
                "failed to write second test payload",
                "failed to flush second test payload",
            ))
            .err()
            .ok_or_else(|| report!("expected muxr pty writer enqueue to fail after writer error"))?;

        assert2::assert!(error.to_string().contains("test pty writer failed"));
        Ok(())
    }

    #[test]
    fn test_run_writer_loop_when_terminal_reply_write_fails_warns() -> rootcause::Result<()> {
        let session = SessionName::default();
        let state = Arc::new(pty_state(&terminal_size()?));
        let (queue, receiver) = self::queued_pty_writer();
        let replies = state.append_output(b"\x1b[6n")?;
        self::write_terminal_replies(&queue, &replies)?;
        queue.shutdown()?;

        let log = crate::session::tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            self::run_writer_loop(&mut *self::failing_pty_writer(), &receiver, queue.state.as_ref());
            Ok(())
        })?;

        assert2::assert!(log.contains("kind=\"pty_writer_stopped_after_error\""));
        assert2::assert!(log.contains("event=\"write_batch\""));
        assert2::assert!(log.contains("session="));
        assert2::assert!(log.contains("test pty writer failed"));
        Ok(())
    }

    #[test]
    fn test_spawn_reader_thread_when_terminal_reply_write_fails_carries_current_span() -> rootcause::Result<()> {
        let session: SessionName = "work".parse()?;
        let state = Arc::new(pty_state(&terminal_size()?));

        let log = crate::session::tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            let (writer, writer_handle) = self::spawn_writer_thread(self::failing_pty_writer());
            let reader_handle = self::spawn_reader_thread(
                Box::new(std::io::Cursor::new(b"\x1b[6n".to_vec())),
                Arc::clone(&state),
                writer.clone(),
            );
            reader_handle
                .join()
                .map_err(|_| report!("muxr pty reader test thread panicked"))?;
            let _shutdown = writer.shutdown();
            writer_handle
                .join()
                .map_err(|_| report!("muxr pty writer test thread panicked"))?;
            Ok(())
        })?;

        assert2::assert!(log.contains("kind=\"pty_writer_stopped_after_error\""));
        assert2::assert!(log.contains("event=\"write_batch\""));
        assert2::assert!(log.contains("session=work"));
        assert2::assert!(log.contains("test pty writer failed"));
        Ok(())
    }

    #[test]
    fn test_write_pty_focus_event_when_focus_reporting_is_disabled_skips_write() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer();
        let written = Arc::new(Mutex::new(Vec::new()));

        self::write_pty_focus_event(&queue, TerminalFocusReporting::Disabled, TerminalFocusEvent::Lost)?;
        self::drain_queued_writes(&queue, &receiver, self::capturing_pty_writer(Arc::clone(&written)))?;

        pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, Vec::<u8>::new());
        Ok(())
    }

    #[test]
    fn test_write_pty_focus_event_when_focus_reporting_is_enabled_writes_event() -> rootcause::Result<()> {
        for (event, expected) in [
            (TerminalFocusEvent::Gained, b"\x1b[I".as_slice()),
            (TerminalFocusEvent::Lost, b"\x1b[O".as_slice()),
        ] {
            let (queue, receiver) = self::queued_pty_writer();
            let written = Arc::new(Mutex::new(Vec::new()));

            self::write_pty_focus_event(&queue, TerminalFocusReporting::Enabled, event)?;
            self::drain_queued_writes(&queue, &receiver, self::capturing_pty_writer(Arc::clone(&written)))?;

            pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, expected.to_vec());
        }
        Ok(())
    }

    #[test]
    fn test_pty_state_with_history_when_output_exists_replays_terminal_state() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let path = tempdir.path().join("1").join("output.raw");
        std::fs::create_dir_all(path.parent().ok_or_else(|| report!("expected history parent"))?)?;
        std::fs::write(&path, b"history").context("failed to write muxr test history")?;

        let state = PtyState::with_history(
            &terminal_size()?,
            &path,
            MuxrConfig::default().scrollback,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        let snapshot = lock_mutex(&state.terminal, "pty terminal")?.snapshot()?;
        let rendered = snapshot
            .rows()
            .iter()
            .flat_map(|row| row.cells().iter().map(muxr_core::RenderCell::text))
            .collect::<String>();

        assert2::assert!(rendered.contains("history"));
        Ok(())
    }

    #[test]
    fn test_shell_cmd_shell_input_line_quotes_shell_words() -> rootcause::Result<()> {
        let cmd = ShellCmd::with_args("/tmp/with space/cmd", ["simple", "two words", "it's"])?;

        pretty_assertions::assert_eq!(
            cmd.shell_input_line(),
            "'/tmp/with space/cmd' simple 'two words' 'it'\\''s'\n"
        );
        Ok(())
    }

    fn terminal_size() -> rootcause::Result<TerminalSize> {
        TerminalSize::new(80, 24)
    }

    fn queued_pty_writer() -> (PtyWriteQueue, mpsc::Receiver<PtyWriteRequest>) {
        self::queued_pty_writer_with_limit(PTY_WRITE_QUEUE_LIMIT)
    }

    fn queued_pty_writer_with_limit(limit: usize) -> (PtyWriteQueue, mpsc::Receiver<PtyWriteRequest>) {
        self::queued_pty_writer_with_limits(limit, PTY_WRITE_QUEUE_BYTE_LIMIT)
    }

    fn queued_pty_writer_with_limits(
        message_limit: usize,
        byte_limit: usize,
    ) -> (PtyWriteQueue, mpsc::Receiver<PtyWriteRequest>) {
        let (sender, receiver) = mpsc::sync_channel(message_limit);
        (
            PtyWriteQueue {
                sender,
                state: Arc::new(PtyWriteState::with_byte_limit(byte_limit)),
            },
            receiver,
        )
    }

    fn drain_queued_writes(
        queue: &PtyWriteQueue,
        receiver: &mpsc::Receiver<PtyWriteRequest>,
        mut writer: Box<dyn Write + Send>,
    ) -> rootcause::Result<()> {
        queue.shutdown()?;
        self::run_writer_loop(&mut *writer, receiver, queue.state.as_ref());
        Ok(())
    }

    fn sink_pty_writer() -> Box<dyn Write + Send> {
        Box::new(std::io::sink())
    }

    fn capturing_pty_writer(written: Arc<Mutex<Vec<u8>>>) -> Box<dyn Write + Send> {
        Box::new(CapturingWriter { written })
    }

    fn failing_pty_writer() -> Box<dyn Write + Send> {
        Box::new(FailingWriter)
    }

    fn captured_pty_bytes(written: &Mutex<Vec<u8>>) -> rootcause::Result<Vec<u8>> {
        Ok(lock_mutex(written, "captured pty bytes")?.clone())
    }

    fn captured_flushes(flushes: &Mutex<usize>) -> rootcause::Result<usize> {
        Ok(*lock_mutex(flushes, "captured pty flushes")?)
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

    struct FlushCountingWriter {
        flushes: Arc<Mutex<usize>>,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for FlushCountingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut written = self
                .written
                .lock()
                .map_err(|_| std::io::Error::other("poisoned muxr flush-counting writer"))?;
            written.extend_from_slice(buf);
            drop(written);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            let mut flushes = self
                .flushes
                .lock()
                .map_err(|_| std::io::Error::other("poisoned muxr flush counter"))?;
            *flushes = flushes
                .checked_add(1)
                .ok_or_else(|| std::io::Error::other("muxr test flush count overflowed"))?;
            drop(flushes);
            Ok(())
        }
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("test pty writer failed"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
