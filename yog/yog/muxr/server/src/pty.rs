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

use muxr_config::ScrollbackConfig;
use muxr_config::ScrollbackDumpStyle;
use muxr_core::ClientMouseEvent;
use muxr_core::PaneMouseMode;
use muxr_core::PaneRegionSnapshot;
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
use crate::terminal::TerminalApplicationMode;
use crate::terminal::TerminalCursorKeyMode;
use crate::terminal::TerminalFocusEvent;
use crate::terminal::TerminalFocusReporting;
use crate::terminal::TerminalMouseProtocol;
use crate::terminal::TerminalSnapshot;
use crate::terminal::TerminalState;

const READ_BUFFER_SIZE: usize = 8192;

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
    handle: PtyHandle,
    reader_handle: Option<thread::JoinHandle<()>>,
}

impl PtySession {
    pub fn spawn(
        cmd: &ShellCmd,
        cwd: &str,
        size: &TerminalSize,
        history_path: &Path,
        scrollback: ScrollbackConfig,
    ) -> rootcause::Result<Self> {
        let state = Arc::new(PtyState::with_history(size, history_path, scrollback)?);
        let pty_pair = native_pty_system()
            .openpty(pty_size(size))
            .map_err(|error| report!("failed to open muxr shell pty").attach(format!("error={error:#}")))?;
        let child = pty_pair
            .slave
            .spawn_command(cmd.cmd_builder(cwd)?)
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
        if self.handle.state.exited.load(Ordering::Acquire) {
            if let Some(reader_handle) = self.reader_handle.take()
                && reader_handle.join().is_err()
            {
                crate::session_tracing::pty::shutdown_failed("join_reader", "reader thread panicked");
            }
            return;
        }

        let mut child = match self::lock_mutex(&self.handle.child, "pty child") {
            Ok(child) => child,
            Err(error) => {
                crate::session_tracing::pty::shutdown_failed("lock_child", &error);
                if let Some(reader_handle) = self.reader_handle.take()
                    && reader_handle.join().is_err()
                {
                    crate::session_tracing::pty::shutdown_failed("join_reader", "reader thread panicked");
                }
                return;
            }
        };

        let exit_status = match child.try_wait() {
            Ok(Some(exit_status)) => Some(exit_status),
            Ok(None) => {
                let _ = child.kill().inspect_err(|error| {
                    crate::session_tracing::pty::shutdown_failed("kill_child", error);
                });
                match child.wait() {
                    Ok(exit_status) => Some(exit_status),
                    Err(error) => {
                        crate::session_tracing::pty::shutdown_failed("wait_child", &error);
                        None
                    }
                }
            }
            Err(error) => {
                crate::session_tracing::pty::shutdown_failed("poll_child", &error);
                let _ = child.kill().inspect_err(|error| {
                    crate::session_tracing::pty::shutdown_failed("kill_child", error);
                });
                None
            }
        };
        if let Some(exit_status) = exit_status {
            let _ = self
                .handle
                .state
                .mark_exited(PtyExitStatus::from(&exit_status))
                .inspect_err(|error| {
                    crate::session_tracing::pty::shutdown_failed("mark_exited", error);
                });
        }
        drop(child);

        if let Some(reader_handle) = self.reader_handle.take()
            && reader_handle.join().is_err()
        {
            crate::session_tracing::pty::shutdown_failed("join_reader", "reader thread panicked");
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
        let framed = crate::terminal::paste_input_bytes(bytes, bracketed_paste_enabled);
        self::write_pty_bytes(
            self.writer.as_ref(),
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
        let Some(bytes) = crate::pane_mouse::encode_pty_mouse_event(event, region, protocol)? else {
            return Ok(None);
        };
        // Scrollback follows only events that reach the PTY, so filtered motion does not hide history.
        let scrolled_to_bottom = lock_mutex(&self.state.terminal, "pty terminal")?.scroll_to_bottom();
        self::write_pty_bytes(
            self.writer.as_ref(),
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
        self.write_input(&crate::pane_scroll::faux_scroll_input_bytes(direction, cursor_key_mode))
    }

    pub fn write_focus_event(&self, event: TerminalFocusEvent) -> rootcause::Result<()> {
        let focus_reporting = self.application_mode()?.focus_reporting;
        self::write_pty_focus_event(self.writer.as_ref(), focus_reporting, event)
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

    pub fn process_id(&self) -> rootcause::Result<Option<u32>> {
        Ok(lock_mutex(&self.child, "pty child")?.process_id())
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
    screen_dirty: AtomicBool,
    terminal: Mutex<TerminalState>,
    title_changes: Mutex<Vec<Option<String>>>,
}

impl PtyState {
    fn with_history(size: &TerminalSize, history_path: &Path, scrollback: ScrollbackConfig) -> rootcause::Result<Self> {
        let (history, replay) = PaneHistory::open(history_path)?;
        let mut terminal = TerminalState::with_scrollback(size, scrollback);
        let _ = terminal.process(&replay);
        // History replay rebuilds visible cells only; metadata and focus-reporting opt-in must come from live PTY
        // output after spawn.
        terminal.clear_title_metadata();
        terminal.clear_replayed_focus_reporting();

        Ok(Self {
            active_sink: Mutex::new(None),
            exited: AtomicBool::new(false),
            exit_status: Mutex::new(None),
            history: Mutex::new(Some(history)),
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

fn write_pty_focus_event(
    writer: &Mutex<Box<dyn Write + Send>>,
    focus_reporting: TerminalFocusReporting,
    event: TerminalFocusEvent,
) -> rootcause::Result<()> {
    match focus_reporting {
        TerminalFocusReporting::Disabled => Ok(()),
        TerminalFocusReporting::Enabled => {
            self::write_pty_bytes(
                writer,
                event.bytes(),
                "failed to write muxr terminal focus event to shell pty",
                "failed to flush muxr terminal focus event",
            )?;
            Ok(())
        }
    }
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
    // Raw OS threads do not inherit thread-local tracing state, so carry both the dispatcher and span explicitly.
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    thread::spawn(move || {
        tracing::dispatcher::with_default(&dispatch, || {
            let _guard = span.enter();
            self::run_reader_loop(&mut *reader, state.as_ref(), writer.as_ref());
        });
    })
}

fn run_reader_loop(reader: &mut dyn Read, state: &PtyState, writer: &Mutex<Box<dyn Write + Send>>) {
    let mut buffer = [0; READ_BUFFER_SIZE];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => {
                // PTY EOF only means the slave side closed; the child may still be running
                // after redirecting stdio, so only child polling is allowed to mark exit.
                break;
            }
            Err(_) => {
                // Read errors stop only the reader loop; child polling still owns exit detection, and a later
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
                        crate::session_tracing::pty::reader_stopped_after_error("append_output", &error);
                        break;
                    }
                };
                if self::write_terminal_replies(writer, &terminal_replies)
                    .inspect_err(|error| {
                        crate::session_tracing::pty::reader_stopped_after_error("write_terminal_replies", error);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;
    use muxr_core::SessionName;

    use super::*;

    fn pty_state(size: &TerminalSize) -> PtyState {
        PtyState {
            active_sink: Mutex::new(None),
            exited: AtomicBool::new(false),
            exit_status: Mutex::new(None),
            history: Mutex::new(None),
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

        let state = PtyState::with_history(&terminal_size()?, &history_path, MuxrConfig::default().scrollback)?;

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

        let state = PtyState::with_history(&terminal_size()?, &history_path, MuxrConfig::default().scrollback)?;
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
        let written = Arc::new(Mutex::new(Vec::new()));
        let writer = self::capturing_pty_writer(Arc::clone(&written));

        let replies = state.append_output(b"\x1b[6n")?;
        self::write_terminal_replies(writer.as_ref(), &replies)?;

        pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, b"\x1b[1;1R".to_vec());
        Ok(())
    }

    #[test]
    fn test_run_reader_loop_when_terminal_reply_write_fails_warns() -> rootcause::Result<()> {
        let session = SessionName::default();
        let state = Arc::new(pty_state(&terminal_size()?));

        let log = crate::session_tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            let mut reader = std::io::Cursor::new(b"\x1b[6n".to_vec());
            let writer = self::failing_pty_writer();
            self::run_reader_loop(&mut reader, state.as_ref(), writer.as_ref());
            Ok(())
        })?;

        assert2::assert!(log.contains("kind=\"pty_reader_stopped_after_error\""));
        assert2::assert!(log.contains("event=\"write_terminal_replies\""));
        assert2::assert!(log.contains("session="));
        assert2::assert!(log.contains("test pty writer failed"));
        Ok(())
    }

    #[test]
    fn test_spawn_reader_thread_when_terminal_reply_write_fails_carries_current_span() -> rootcause::Result<()> {
        let session: SessionName = "work".parse()?;
        let state = Arc::new(pty_state(&terminal_size()?));

        let log = crate::session_tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            let reader_handle = self::spawn_reader_thread(
                Box::new(std::io::Cursor::new(b"\x1b[6n".to_vec())),
                Arc::clone(&state),
                self::failing_pty_writer(),
            );
            reader_handle
                .join()
                .map_err(|_| report!("muxr pty reader test thread panicked"))?;
            Ok(())
        })?;

        assert2::assert!(log.contains("kind=\"pty_reader_stopped_after_error\""));
        assert2::assert!(log.contains("event=\"write_terminal_replies\""));
        assert2::assert!(log.contains("session=work"));
        assert2::assert!(log.contains("test pty writer failed"));
        Ok(())
    }

    #[test]
    fn test_write_pty_focus_event_when_focus_reporting_is_disabled_skips_write() -> rootcause::Result<()> {
        let written = Arc::new(Mutex::new(Vec::new()));
        let writer = self::capturing_pty_writer(Arc::clone(&written));

        self::write_pty_focus_event(
            writer.as_ref(),
            TerminalFocusReporting::Disabled,
            TerminalFocusEvent::Lost,
        )?;

        pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, Vec::<u8>::new());
        Ok(())
    }

    #[test]
    fn test_write_pty_focus_event_when_focus_reporting_is_enabled_writes_event() -> rootcause::Result<()> {
        for (event, expected) in [
            (TerminalFocusEvent::Gained, b"\x1b[I".as_slice()),
            (TerminalFocusEvent::Lost, b"\x1b[O".as_slice()),
        ] {
            let written = Arc::new(Mutex::new(Vec::new()));
            let writer = self::capturing_pty_writer(Arc::clone(&written));

            self::write_pty_focus_event(writer.as_ref(), TerminalFocusReporting::Enabled, event)?;

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

        let state = PtyState::with_history(&terminal_size()?, &path, MuxrConfig::default().scrollback)?;
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

    fn sink_pty_writer() -> Arc<Mutex<Box<dyn Write + Send>>> {
        Arc::new(Mutex::new(Box::new(std::io::sink())))
    }

    fn capturing_pty_writer(written: Arc<Mutex<Vec<u8>>>) -> Arc<Mutex<Box<dyn Write + Send>>> {
        Arc::new(Mutex::new(Box::new(CapturingWriter { written })))
    }

    fn failing_pty_writer() -> Arc<Mutex<Box<dyn Write + Send>>> {
        Arc::new(Mutex::new(Box::new(FailingWriter)))
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
