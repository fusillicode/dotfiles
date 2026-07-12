use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;

use kanal::SendError;
use kanal::Sender;
use muxr_config::ScrollbackConfig;
use muxr_config::ScrollbackDumpStyle;
use muxr_core::ClientMouseEvent;
use muxr_core::PaneMouseMode;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneScrollDirection;
use muxr_core::TerminalSize;
use parking_lot::Mutex;
use portable_pty::Child;
use portable_pty::ChildKiller;
use portable_pty::MasterPty;
use portable_pty::PtySize;
use portable_pty::native_pty_system;
use rootcause::prelude::ResultExt;
use rootcause::report;

use super::cmd::ShellCmd;
use super::event::PtyEvent;
use super::event::PtyExitStatus;
use super::writer;
use super::writer::PtyWriter;
use crate::history::PaneHistory;
use crate::render_state::OutputFreshness;
use crate::terminal::TerminalApplicationMode;
use crate::terminal::TerminalCursorKeyMode;
use crate::terminal::TerminalFocusEvent;
use crate::terminal::TerminalMouseProtocol;
use crate::terminal::TerminalReplies;
use crate::terminal::TerminalScrollMove;
use crate::terminal::TerminalSnapshot;
use crate::terminal::TerminalState;

const READ_BUFFER_SIZE: usize = 8192;

type PtyChild = Box<dyn Child + Send + Sync>;
type SharedPtyChild = Arc<Mutex<Option<PtyChild>>>;

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
        let mut child = pty_pair
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

        let (writer, writer_handle) = match writer::spawn(writer) {
            Ok(writer_parts) => writer_parts,
            Err(error) => {
                self::kill_and_wait_child(child.as_mut());
                return Err(error);
            }
        };
        let handle = PtyHandle {
            child_killer,
            child_process_id,
            master: Arc::new(Mutex::new(pty_pair.master)),
            state: Arc::clone(&state),
            writer: writer.clone(),
        };
        let mut session = Self {
            child_wait_handle: None,
            handle,
            reader_handle: None,
            writer_handle: Some(writer_handle),
        };
        // Own started resources before each later fallible thread spawn so normal drop cleanup runs on those error
        // paths.
        session.child_wait_handle = Some(spawn_child_wait_thread(child, &state)?);
        session.reader_handle = Some(spawn_reader_thread(reader, state, writer)?);

        Ok(session)
    }

    pub fn handle(&self) -> PtyHandle {
        self.handle.clone()
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        if !self.handle.state.exited.load(Ordering::Acquire) {
            self::kill_child(&self.handle.child_killer);
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
pub struct PtyHandle {
    child_killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    child_process_id: Option<u32>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    state: Arc<PtyState>,
    writer: PtyWriter,
}

/// Pane render data captured under one terminal-state lock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PtyRenderSnapshot {
    mouse_mode: muxr_core::PaneMouseMode,
    terminal: TerminalSnapshot,
    visible_row_wraps: Vec<muxr_core::RowWrap>,
    visible_top_row: u64,
}

impl PtyRenderSnapshot {
    pub const fn mouse_mode(&self) -> muxr_core::PaneMouseMode {
        self.mouse_mode
    }

    pub const fn terminal(&self) -> &TerminalSnapshot {
        &self.terminal
    }

    pub fn visible_row_wraps(&self) -> &[muxr_core::RowWrap] {
        &self.visible_row_wraps
    }

    pub const fn visible_top_row(&self) -> u64 {
        self.visible_top_row
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PtyViewportMove {
    MovedToBottom,
    #[default]
    Unchanged,
}

impl From<TerminalScrollMove> for PtyViewportMove {
    fn from(value: TerminalScrollMove) -> Self {
        match value {
            TerminalScrollMove::Moved => Self::MovedToBottom,
            TerminalScrollMove::Unchanged => Self::Unchanged,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PtyMouseWrite {
    Ignored,
    Sent(PtyViewportMove),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PtyScreenDmg {
    Dirty,
    #[default]
    Clean,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PtyExitState {
    Exited,
    Running,
}

impl PtyHandle {
    pub fn attach_sink(&self, sender: Sender<PtyEvent>) -> PtySinkGuard {
        self.state.attach_sink(sender)
    }

    pub fn exit_state(&self) -> PtyExitState {
        if self.state.exited.load(Ordering::Acquire) {
            PtyExitState::Exited
        } else {
            PtyExitState::Running
        }
    }

    pub fn resize(&self, size: &TerminalSize) -> rootcause::Result<()> {
        self.master
            .lock()
            .resize(pty_size(size))
            .map_err(|error| report!("failed to resize muxr shell pty").attach(format!("error={error:#}")))?;
        self.state.terminal.lock().resize(size);
        Ok(())
    }

    pub fn write_input(&self, bytes: &[u8]) -> rootcause::Result<PtyViewportMove> {
        if bytes.is_empty() {
            return Ok(PtyViewportMove::Unchanged);
        }

        // PTY-bound input should reveal the live viewport before an app echoes typed bytes; some apps do not echo, so
        // callers need the changed flag to redraw immediately after resetting scrollback.
        let viewport_move = PtyViewportMove::from(self.state.terminal.lock().scroll_to_bottom());
        self.writer.write_bytes(
            bytes,
            "failed to write client input to muxr shell pty",
            "failed to flush muxr shell pty input",
        )?;
        Ok(viewport_move)
    }

    pub fn write_paste(&self, bytes: &[u8]) -> rootcause::Result<PtyViewportMove> {
        if bytes.is_empty() {
            return Ok(PtyViewportMove::Unchanged);
        }

        let (viewport_move, paste_mode) = {
            let mut terminal = self.state.terminal.lock();
            let viewport_move = PtyViewportMove::from(terminal.scroll_to_bottom());
            (viewport_move, terminal.paste_mode())
        };
        let framed = crate::terminal::paste_input_bytes(bytes, paste_mode);
        self.writer.write_bytes(
            &framed,
            "failed to write client paste to muxr shell pty",
            "failed to flush muxr shell pty paste",
        )?;
        Ok(viewport_move)
    }

    pub fn write_mouse_event(
        &self,
        event: ClientMouseEvent,
        region: &PaneRegionSnapshot,
        protocol: TerminalMouseProtocol,
    ) -> rootcause::Result<PtyMouseWrite> {
        let Some(bytes) = crate::pane::mouse::encode_pty_mouse_event(event, region, protocol)? else {
            return Ok(PtyMouseWrite::Ignored);
        };
        // Scrollback follows only events that reach the PTY, so filtered motion does not hide history.
        let viewport_move = PtyViewportMove::from(self.state.terminal.lock().scroll_to_bottom());
        self.writer.write_bytes(
            &bytes,
            "failed to write client mouse event to muxr shell pty",
            "failed to flush muxr shell pty mouse event",
        )?;
        Ok(PtyMouseWrite::Sent(viewport_move))
    }

    pub fn write_faux_scroll_input(
        &self,
        direction: PaneScrollDirection,
        cursor_key_mode: TerminalCursorKeyMode,
    ) -> rootcause::Result<PtyViewportMove> {
        self.write_input(&crate::pane::scroll::faux_scroll_input_bytes(
            direction,
            cursor_key_mode,
        ))
    }

    pub fn write_focus_event(&self, event: TerminalFocusEvent) -> rootcause::Result<()> {
        let focus_reporting = self.application_mode().focus_reporting;
        self.writer.write_focus_event(focus_reporting, event)
    }

    pub fn mouse_mode(&self) -> PaneMouseMode {
        self.application_mode().pane_mouse_mode()
    }

    pub fn application_mode(&self) -> TerminalApplicationMode {
        self.state.terminal.lock().application_mode()
    }

    pub fn scroll(&self, direction: PaneScrollDirection) -> TerminalScrollMove {
        self.state.terminal.lock().scroll(direction)
    }

    pub fn scroll_one_line(&self, direction: PaneScrollDirection) -> TerminalScrollMove {
        self.state.terminal.lock().scroll_one_line(direction)
    }

    pub fn visible_top_row(&self) -> rootcause::Result<u64> {
        self.state.terminal.lock().visible_top_row()
    }

    pub fn visible_row_wraps(&self) -> Vec<muxr_core::RowWrap> {
        self.state.terminal.lock().visible_row_wraps()
    }

    pub fn exit_status(&self) -> Option<PtyExitStatus> {
        self.state.exit_status.lock().clone()
    }

    pub const fn process_id(&self) -> Option<u32> {
        self.child_process_id
    }

    pub fn fg_process_group(&self) -> Option<u32> {
        self.master
            .lock()
            .process_group_leader()
            .and_then(|process_group| u32::try_from(process_group).ok())
            .filter(|process_group| *process_group != 0)
    }

    pub fn terminal_title(&self) -> Option<String> {
        self.state.terminal.lock().title()
    }

    pub fn take_title_changes(&self) -> Vec<Option<String>> {
        self.state.take_title_changes()
    }

    pub fn take_screen_dirty(&self) -> PtyScreenDmg {
        self.state.take_screen_dirty()
    }

    #[cfg(test)]
    pub fn render_snapshot(&self) -> rootcause::Result<TerminalSnapshot> {
        self.state.terminal.lock().snapshot()
    }

    pub fn pane_render_snapshot(&self) -> rootcause::Result<PtyRenderSnapshot> {
        let terminal = self.state.terminal.lock();
        Ok(PtyRenderSnapshot {
            mouse_mode: terminal.application_mode().pane_mouse_mode(),
            visible_top_row: terminal.visible_top_row()?,
            visible_row_wraps: terminal.visible_row_wraps(),
            terminal: terminal.snapshot()?,
        })
    }

    pub fn write_scrollback_dump(&self, style: ScrollbackDumpStyle, writer: &mut impl Write) -> rootcause::Result<()> {
        let dump = self
            .state
            .terminal
            .lock()
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
    /// Return current while full `OutputReady` events are coalesced, and stale after the live sink disconnects.
    pub fn output_freshness(&self) -> OutputFreshness {
        if self.output_current.load(Ordering::Acquire) {
            OutputFreshness::Current
        } else {
            OutputFreshness::Stale
        }
    }
}

impl Drop for PtySinkGuard {
    fn drop(&mut self) {
        *self.state.active_sink.lock() = None;
    }
}

struct ActivePtySink {
    output_current: Arc<AtomicBool>,
    sender: Sender<PtyEvent>,
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

    fn attach_sink(self: &Arc<Self>, sender: Sender<PtyEvent>) -> PtySinkGuard {
        let output_current = Arc::new(AtomicBool::new(true));
        self.title_changes.lock().clear();
        // Attach sends a fresh baseline; discard dirty state accumulated before the client could observe output events.
        self.screen_dirty.store(false, Ordering::Release);
        *self.active_sink.lock() = Some(ActivePtySink {
            output_current: Arc::clone(&output_current),
            sender,
        });

        PtySinkGuard {
            output_current,
            state: Arc::clone(self),
        }
    }

    fn append_output(&self, bytes: &[u8]) -> rootcause::Result<TerminalReplies> {
        if let Some(history) = self.history.lock().as_mut() {
            history.append(bytes)?;
        }
        let terminal_replies = {
            let mut terminal = self.terminal.lock();
            let process_outcome = terminal.process(bytes);
            if process_outcome.screen_dmg() == crate::terminal::TerminalScreenDmg::Dirty {
                // Output events are coalesced, so the visible-screen dirty bit must be sticky until the server consumes
                // it.
                self.screen_dirty.store(true, Ordering::Release);
            }
            let terminal_replies = process_outcome.into_replies();
            let title_changes = terminal.take_title_changes();
            drop(terminal);
            // Title changes are queued separately from coalesced output events so cmd->cwd title transitions are
            // not collapsed before the server can emit matching tab bar updates.
            let active_sink = self.active_sink.lock();
            if !title_changes.is_empty() && active_sink.is_some() {
                self.title_changes.lock().extend(title_changes);
            }
            drop(active_sink);
            terminal_replies
        };

        let mut active_sink = self.active_sink.lock();
        if let Some(sink) = active_sink.as_ref() {
            match self::try_send_pty_event(&sink.sender, PtyEvent::OutputReady)? {
                PtyEventSendOutcome::Sent | PtyEventSendOutcome::Full(PtyEvent::OutputReady) => {}
                PtyEventSendOutcome::Disconnected(PtyEvent::OutputReady) => {
                    sink.output_current.store(false, Ordering::Release);
                    *active_sink = None;
                }
                PtyEventSendOutcome::Full(PtyEvent::Exited) | PtyEventSendOutcome::Disconnected(PtyEvent::Exited) => {
                    return Err(report!("unexpected muxr pty exit event while sending output"));
                }
            }
        }
        drop(active_sink);

        Ok(terminal_replies)
    }

    fn take_title_changes(&self) -> Vec<Option<String>> {
        let mut title_changes = self.title_changes.lock();
        std::mem::take(&mut *title_changes)
    }

    fn take_screen_dirty(&self) -> PtyScreenDmg {
        if self.screen_dirty.swap(false, Ordering::AcqRel) {
            PtyScreenDmg::Dirty
        } else {
            PtyScreenDmg::Clean
        }
    }

    fn mark_exited(&self, exit_status: PtyExitStatus) -> rootcause::Result<()> {
        let mut stored_exit_status = self.exit_status.lock();
        if stored_exit_status.is_none() {
            *stored_exit_status = Some(exit_status);
        }
        drop(stored_exit_status);

        self.exited.store(true, Ordering::Release);
        // Detached sessions have no active PTY sink; notify the server loop so it can reap sticky exit state.
        self.pane_exit_notify.notify_one();

        let mut active_sink = self.active_sink.lock();
        if let Some(sink) = active_sink.as_ref() {
            match self::try_send_pty_event(&sink.sender, PtyEvent::Exited)? {
                PtyEventSendOutcome::Sent => {}
                PtyEventSendOutcome::Full(_) => {
                    crate::session::tracing::pty::exit_wakeup_not_queued("channel_full");
                }
                PtyEventSendOutcome::Disconnected(_) => {
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

enum PtyEventSendOutcome {
    Disconnected(PtyEvent),
    Full(PtyEvent),
    Sent,
}

// `kanal::try_send` reports a full queue as `Ok(false)` and drops the event. Keep the event explicit so
// `OutputReady` coalescing and `Exited` best-effort wakeups remain auditable at muxr's PTY boundary.
fn try_send_pty_event(sender: &Sender<PtyEvent>, event: PtyEvent) -> rootcause::Result<PtyEventSendOutcome> {
    let mut pending = Some(event);
    match sender.try_send_option(&mut pending) {
        Ok(true) => Ok(PtyEventSendOutcome::Sent),
        Ok(false) => Ok(PtyEventSendOutcome::Full(self::pending_pty_event(pending)?)),
        Err(SendError::Closed | SendError::ReceiveClosed) => {
            Ok(PtyEventSendOutcome::Disconnected(self::pending_pty_event(pending)?))
        }
    }
}

fn pending_pty_event(pending: Option<PtyEvent>) -> rootcause::Result<PtyEvent> {
    pending.ok_or_else(|| report!("kanal dropped muxr pty event during failed send"))
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
    writer: PtyWriter,
) -> rootcause::Result<thread::JoinHandle<()>> {
    // Raw OS threads do not inherit thread-local tracing state, so carry both the dispatcher and span explicitly.
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    Ok(thread::Builder::new()
        .name("muxr-pty-reader".to_owned())
        .spawn(move || {
            tracing::dispatcher::with_default(&dispatch, || {
                let _guard = span.enter();
                self::run_reader_loop(&mut *reader, state.as_ref(), &writer);
            });
        })
        .context("failed to spawn muxr pty reader thread")?)
}

fn kill_child(child_killer: &Mutex<Box<dyn ChildKiller + Send + Sync>>) {
    let _ = child_killer.lock().kill().inspect_err(|error| {
        crate::session::tracing::pty::shutdown_failed("kill_child", error);
    });
}

fn kill_and_wait_child(child: &mut dyn Child) -> Option<PtyExitStatus> {
    // Early setup failures can happen before the child wait thread owns the child, so cleanup must reap the killed
    // process here instead of relying on the normal wait-thread path.
    let _ = child.kill().inspect_err(|error| {
        crate::session::tracing::pty::shutdown_failed("kill_child", error);
    });
    match child.wait() {
        Ok(exit_status) => Some(PtyExitStatus::from(&exit_status)),
        Err(error) => {
            crate::session::tracing::pty::shutdown_failed("wait_child_after_kill", &error);
            None
        }
    }
}

fn take_child_wait_handle(child: &SharedPtyChild) -> Option<PtyChild> {
    child.lock().take()
}

fn spawn_child_wait_thread(child: PtyChild, state: &Arc<PtyState>) -> rootcause::Result<thread::JoinHandle<()>> {
    // Raw OS threads do not inherit thread-local tracing state, so carry both the dispatcher and span explicitly.
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    // Keep the child recoverable until the wait thread is successfully spawned. Exactly one side takes it: the wait
    // thread on success, or the spawn-error cleanup path before any reaper exists.
    let child = Arc::new(Mutex::new(Some(child)));
    let thread_child = Arc::clone(&child);
    let thread_state = Arc::clone(state);
    match thread::Builder::new()
        .name("muxr-pty-child-wait".to_owned())
        .spawn(move || {
            tracing::dispatcher::with_default(&dispatch, || {
                let _guard = span.enter();
                let Some(mut child) = self::take_child_wait_handle(&thread_child) else {
                    return;
                };
                match child.wait() {
                    Ok(exit_status) => {
                        let _ = thread_state
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
        }) {
        Ok(handle) => Ok(handle),
        Err(error) => {
            if let Some(mut child) = self::take_child_wait_handle(&child)
                && let Some(exit_status) = self::kill_and_wait_child(child.as_mut())
            {
                let _ = state.mark_exited(exit_status).inspect_err(|error| {
                    crate::session::tracing::pty::shutdown_failed("mark_exited", error);
                });
            }
            Err(report!("failed to spawn muxr pty child wait thread").attach(format!("error={error:#}")))
        }
    }
}

fn run_reader_loop(reader: &mut dyn Read, state: &PtyState, writer: &PtyWriter) {
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
                if writer
                    .write_terminal_replies(terminal_replies.as_slice())
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use muxr_config::MuxrConfig;
    use muxr_core::SessionName;
    use test_that::prelude::*;

    use super::super::event::PtyExitResult;
    use super::*;
    use crate::terminal::TerminalFocusReporting;

    fn assert_replies_eq(replies: &TerminalReplies, expected: &[Vec<u8>]) {
        assert_that!(replies.as_slice(), eq(expected));
    }

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

    #[derive(Debug)]
    struct RecordingChild {
        events: Arc<Mutex<Vec<&'static str>>>,
        exit_status: portable_pty::ExitStatus,
    }

    impl RecordingChild {
        fn new(events: Arc<Mutex<Vec<&'static str>>>, exit_code: u32) -> Self {
            Self {
                events,
                exit_status: portable_pty::ExitStatus::with_exit_code(exit_code),
            }
        }
    }

    impl ChildKiller for RecordingChild {
        fn kill(&mut self) -> std::io::Result<()> {
            self.events.lock().push("kill");
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(RecordingChildKiller {
                events: Arc::clone(&self.events),
            })
        }
    }

    impl Child for RecordingChild {
        fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
            Ok(Some(self.exit_status.clone()))
        }

        fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
            self.events.lock().push("wait");
            Ok(self.exit_status.clone())
        }

        fn process_id(&self) -> Option<u32> {
            Some(42)
        }

        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
            None
        }
    }

    #[derive(Debug)]
    struct RecordingChildKiller {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    impl ChildKiller for RecordingChildKiller {
        fn kill(&mut self) -> std::io::Result<()> {
            self.events.lock().push("clone_kill");
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(Self {
                events: Arc::clone(&self.events),
            })
        }
    }

    #[test]
    fn test_kill_and_wait_child_when_child_exits_reaps_and_returns_exit_status() -> rootcause::Result<()> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut child = RecordingChild::new(Arc::clone(&events), 7);

        let exit_status = self::kill_and_wait_child(&mut child).ok_or_else(|| report!("expected child exit status"))?;

        assert_that!(*events.lock(), eq(vec!["kill", "wait"]));
        assert_that!(
            exit_status,
            eq(PtyExitStatus {
                code: 7,
                result: PtyExitResult::Failed,
                signal: None,
            })
        );
        Ok(())
    }

    #[test]
    fn test_spawn_child_wait_thread_when_child_exits_marks_exit_status() -> rootcause::Result<()> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let state = Arc::new(pty_state(&terminal_size()?));
        let child_wait_handle =
            self::spawn_child_wait_thread(Box::new(RecordingChild::new(Arc::clone(&events), 7)), &state)?;

        child_wait_handle
            .join()
            .map_err(|_| report!("muxr pty child wait test thread panicked"))?;

        assert_that!(*events.lock(), eq(vec!["wait"]));
        assert_that!(state.exited.load(Ordering::Acquire), eq(true));
        assert_that!(
            *state.exit_status.lock(),
            eq(Some(PtyExitStatus {
                code: 7,
                result: PtyExitResult::Failed,
                signal: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_spawn_reader_thread_when_pty_reaches_eof_does_not_mark_child_exited() -> rootcause::Result<()> {
        let state = Arc::new(pty_state(&terminal_size()?));
        let (writer, writer_handle) = writer::spawn(self::sink_pty_writer())?;
        let reader_handle = spawn_reader_thread(
            Box::new(std::io::Cursor::new(Vec::new())),
            Arc::clone(&state),
            writer.clone(),
        )?;

        reader_handle
            .join()
            .map_err(|_| report!("muxr pty reader test thread panicked"))?;
        writer.shutdown()?;
        writer_handle
            .join()
            .map_err(|_| report!("muxr pty writer test thread panicked"))?;

        assert_that!(state.exited.load(Ordering::Acquire), eq(false));
        assert_that!(state.exit_status.lock().as_ref(), none());
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
        let (sender, receiver) = kanal::bounded(1);
        let _guard = handle.attach_sink(sender);

        assert_that!(
            receiver.recv_timeout(Duration::from_secs(2)),
            ok(matches_pattern!(PtyEvent::Exited))
        );
        assert_that!(
            handle.exit_status(),
            eq(Some(PtyExitStatus {
                code: 7,
                signal: None,
                result: PtyExitResult::Failed,
            }))
        );
        drop(session);
        Ok(())
    }

    #[test]
    fn test_pty_state_mark_exited_when_exit_wakeup_channel_is_full_warns_and_keeps_sticky_state()
    -> rootcause::Result<()> {
        let state = pty_state(&terminal_size()?);
        let output_current = Arc::new(AtomicBool::new(true));
        let (sender, _receiver) = kanal::bounded(0);
        *state.active_sink.lock() = Some(ActivePtySink {
            output_current: Arc::clone(&output_current),
            sender,
        });
        let session = SessionName::default();

        let log = crate::session::tracing::collect_test_log(&session, || {
            state.mark_exited(PtyExitStatus {
                code: 7,
                signal: None,
                result: PtyExitResult::Failed,
            })
        })?;

        assert_that!(state.exited.load(Ordering::Acquire), eq(true));
        assert_that!(output_current.load(Ordering::Acquire), eq(true));
        assert_that!(state.active_sink.lock().as_ref().map(|_| ()), some(eq(())));
        assert_that!(
            *state.exit_status.lock(),
            eq(Some(PtyExitStatus {
                code: 7,
                signal: None,
                result: PtyExitResult::Failed,
            }))
        );
        assert_that!(log, contains_substring("kind=\"pty_exit_wakeup_not_queued\""));
        assert_that!(log, contains_substring("reason=\"channel_full\""));
        Ok(())
    }

    #[test]
    fn test_pty_state_mark_exited_when_sink_receiver_is_dropped_clears_active_sink() -> rootcause::Result<()> {
        let state = pty_state(&terminal_size()?);
        let output_current = Arc::new(AtomicBool::new(true));
        let (sender, receiver) = kanal::bounded(1);
        *state.active_sink.lock() = Some(ActivePtySink {
            output_current: Arc::clone(&output_current),
            sender,
        });
        drop(receiver);
        let session = SessionName::default();

        let log = crate::session::tracing::collect_test_log(&session, || {
            state.mark_exited(PtyExitStatus {
                code: 7,
                signal: None,
                result: PtyExitResult::Failed,
            })
        })?;

        assert_that!(state.exited.load(Ordering::Acquire), eq(true));
        assert_that!(state.active_sink.lock().as_ref().map(|_| ()), none());
        assert_that!(output_current.load(Ordering::Acquire), eq(false));
        assert_that!(
            *state.exit_status.lock(),
            eq(Some(PtyExitStatus {
                code: 7,
                signal: None,
                result: PtyExitResult::Failed,
            }))
        );
        assert_that!(log, contains_substring("kind=\"pty_exit_wakeup_not_queued\""));
        assert_that!(log, contains_substring("reason=\"channel_disconnected\""));
        Ok(())
    }

    #[test]
    fn test_attach_sink_when_output_arrives_after_attach_delivers_live_event() -> rootcause::Result<()> {
        let state = Arc::new(pty_state(&terminal_size()?));
        self::assert_replies_eq(&(state.append_output(b"before")?), &[]);
        let (sender, receiver) = kanal::bounded(1);

        let _guard = state.attach_sink(sender);
        self::assert_replies_eq(&(state.append_output(b"after")?), &[]);

        assert_that!(receiver.recv(), ok(eq(PtyEvent::OutputReady)));
        Ok(())
    }

    #[test]
    fn test_append_output_when_sink_receiver_is_dropped_clears_active_sink() -> rootcause::Result<()> {
        let state = pty_state(&terminal_size()?);
        let output_current = Arc::new(AtomicBool::new(true));
        let (sender, receiver) = kanal::bounded(1);
        *state.active_sink.lock() = Some(ActivePtySink {
            output_current: Arc::clone(&output_current),
            sender,
        });
        drop(receiver);

        self::assert_replies_eq(&(state.append_output(b"lost client")?), &[]);

        assert_that!(state.active_sink.lock().as_ref().map(|_| ()), none());
        assert_that!(output_current.load(Ordering::Acquire), eq(false));
        Ok(())
    }

    #[test]
    fn test_attach_sink_when_output_arrived_before_attach_clears_screen_dirty() -> rootcause::Result<()> {
        let state = Arc::new(pty_state(&terminal_size()?));
        self::assert_replies_eq(&(state.append_output(b"before")?), &[]);
        assert_that!(state.take_screen_dirty(), eq(PtyScreenDmg::Dirty));
        self::assert_replies_eq(&(state.append_output(b"before again")?), &[]);
        let (sender, _receiver) = kanal::bounded(1);

        let _guard = state.attach_sink(sender);

        assert_that!(state.take_screen_dirty(), eq(PtyScreenDmg::Clean));
        Ok(())
    }

    #[test]
    fn test_append_output_when_title_only_changes_does_not_mark_screen_dirty() -> rootcause::Result<()> {
        let state = Arc::new(pty_state(&terminal_size()?));
        let (sender, receiver) = kanal::bounded(1);
        let _guard = state.attach_sink(sender);

        self::assert_replies_eq(&(state.append_output(b"\x1b]2;~\x07")?), &[]);

        assert_that!(state.take_screen_dirty(), eq(PtyScreenDmg::Clean));
        assert_that!(state.take_title_changes(), eq(vec![Some("~".to_owned())]));
        assert_that!(receiver.recv(), ok(eq(PtyEvent::OutputReady)));
        Ok(())
    }

    #[test]
    fn test_append_output_when_visible_output_arrives_marks_screen_dirty_until_taken() -> rootcause::Result<()> {
        let state = pty_state(&terminal_size()?);

        self::assert_replies_eq(&(state.append_output(b"visible")?), &[]);

        assert_that!(state.take_screen_dirty(), eq(PtyScreenDmg::Dirty));
        assert_that!(state.take_screen_dirty(), eq(PtyScreenDmg::Clean));
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

        assert_that!(state.terminal.lock().title(), eq(None));
        assert_that!(state.take_title_changes(), eq(Vec::<Option<String>>::new()));
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
        let mode = state.terminal.lock().application_mode();

        assert_that!(mode.focus_reporting, eq(TerminalFocusReporting::Disabled));
        Ok(())
    }

    #[test]
    fn test_append_output_when_sink_is_full_coalesces_without_blocking() -> rootcause::Result<()> {
        let state = pty_state(&terminal_size()?);
        let (sender, receiver) = kanal::bounded(1);
        let output_current = Arc::new(AtomicBool::new(true));
        *state.active_sink.lock() = Some(ActivePtySink {
            output_current: Arc::clone(&output_current),
            sender,
        });

        self::assert_replies_eq(&(state.append_output(b"first")?), &[]);
        self::assert_replies_eq(&(state.append_output(b"second")?), &[]);

        assert_that!(state.active_sink.lock().as_ref().map(|_| ()), some(eq(())));
        assert_that!(output_current.load(Ordering::Acquire), eq(true));
        assert_that!(state.take_screen_dirty(), eq(PtyScreenDmg::Dirty));
        assert_that!(state.take_screen_dirty(), eq(PtyScreenDmg::Clean));
        assert_that!(receiver.recv(), ok(eq(PtyEvent::OutputReady)));
        Ok(())
    }

    #[test]
    fn test_append_output_when_sink_is_full_and_title_changes_preserves_title_changes() -> rootcause::Result<()> {
        let state = pty_state(&terminal_size()?);
        let (sender, receiver) = kanal::bounded(1);
        let output_current = Arc::new(AtomicBool::new(true));
        *state.active_sink.lock() = Some(ActivePtySink {
            output_current: Arc::clone(&output_current),
            sender,
        });

        self::assert_replies_eq(&(state.append_output(b"first")?), &[]);
        self::assert_replies_eq(&(state.append_output(b"\x1b]2;cargo test\x07\x1b]2;~\x07")?), &[]);

        assert_that!(
            state.take_title_changes(),
            eq(vec![Some("cargo test".to_owned()), Some("~".to_owned())])
        );
        assert_that!(receiver.recv(), ok(eq(PtyEvent::OutputReady)));
        Ok(())
    }

    #[test]
    fn test_spawn_reader_thread_when_terminal_reply_write_fails_carries_current_span() -> rootcause::Result<()> {
        let session: SessionName = "work".parse()?;
        let state = Arc::new(pty_state(&terminal_size()?));

        let log = crate::session::tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            let (writer, writer_handle) = writer::spawn(self::failing_pty_writer())?;
            let reader_handle = self::spawn_reader_thread(
                Box::new(std::io::Cursor::new(b"\x1b[6n".to_vec())),
                Arc::clone(&state),
                writer.clone(),
            )?;
            reader_handle
                .join()
                .map_err(|_| report!("muxr pty reader test thread panicked"))?;
            let _shutdown = writer.shutdown();
            writer_handle
                .join()
                .map_err(|_| report!("muxr pty writer test thread panicked"))?;
            Ok(())
        })?;

        assert_that!(log, contains_substring("kind=\"pty_writer_stopped_after_error\""));
        assert_that!(log, contains_substring("event=\"write_batch\""));
        assert_that!(log, contains_substring("session=work"));
        assert_that!(log, contains_substring("test pty writer failed"));
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
        let snapshot = state.terminal.lock().snapshot()?;
        let rendered = snapshot
            .rows()
            .iter()
            .flat_map(|row| row.cells().iter().map(muxr_core::RenderCell::text))
            .collect::<String>();

        assert_that!(rendered, contains_substring("history"));
        Ok(())
    }

    fn terminal_size() -> rootcause::Result<TerminalSize> {
        TerminalSize::new(80, 24)
    }

    fn sink_pty_writer() -> Box<dyn Write + Send> {
        Box::new(std::io::sink())
    }

    fn failing_pty_writer() -> Box<dyn Write + Send> {
        Box::new(FailingWriter)
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
