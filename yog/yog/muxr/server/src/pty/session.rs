use std::io::Read;
use std::io::Write;
use std::path::Path;
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
use crate::terminal::TerminalApplicationMode;
use crate::terminal::TerminalCursorKeyMode;
use crate::terminal::TerminalFocusEvent;
use crate::terminal::TerminalMouseProtocol;
use crate::terminal::TerminalSnapshot;
use crate::terminal::TerminalState;

const READ_BUFFER_SIZE: usize = 8192;

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

        let (writer, writer_handle) = writer::spawn(writer);
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
pub struct PtyHandle {
    child_killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    child_process_id: Option<u32>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    state: Arc<PtyState>,
    writer: PtyWriter,
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
        self.writer.write_bytes(
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
        self.writer.write_bytes(
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
        self.writer.write_bytes(
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
        self.writer.write_focus_event(focus_reporting, event)
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

fn lock_mutex<'a, T>(mutex: &'a Mutex<T>, name: &str) -> rootcause::Result<MutexGuard<'a, T>> {
    mutex.lock().map_err(|_| report!("poisoned muxr {name} mutex"))
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
                    .write_terminal_replies(&terminal_replies)
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

    use super::*;
    use crate::terminal::TerminalFocusReporting;

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
    fn test_spawn_reader_thread_when_pty_reaches_eof_does_not_mark_child_exited() -> rootcause::Result<()> {
        let state = Arc::new(pty_state(&terminal_size()?));
        let (writer, writer_handle) = writer::spawn(self::sink_pty_writer());
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
    fn test_spawn_reader_thread_when_terminal_reply_write_fails_carries_current_span() -> rootcause::Result<()> {
        let session: SessionName = "work".parse()?;
        let state = Arc::new(pty_state(&terminal_size()?));

        let log = crate::session::tracing::collect_test_log(&session, || {
            let span = tracing::info_span!("muxr_session", session = %session);
            let _guard = span.enter();
            let (writer, writer_handle) = writer::spawn(self::failing_pty_writer());
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
