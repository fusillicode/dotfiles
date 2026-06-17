use std::io::Write;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::mpsc;
use std::sync::mpsc::TrySendError;
use std::thread;

use rootcause::Report;
use rootcause::markers::Cloneable;
use rootcause::markers::Dynamic;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::terminal::TerminalFocusEvent;
use crate::terminal::TerminalFocusReporting;

const PTY_WRITE_QUEUE_LIMIT: usize = 1024;
const PTY_WRITE_QUEUE_BYTE_LIMIT: usize = 1024 * 1024;
const PTY_WRITE_BATCH_MAX_MESSAGES: usize = 64;
const PTY_WRITE_BATCH_MAX_BYTES: usize = 64 * 1024;
const PTY_WRITE_MAX_MESSAGE_BYTES: usize = PTY_WRITE_BATCH_MAX_BYTES;

type PtyWriterError = Report<Dynamic, Cloneable>;

#[derive(Clone)]
pub struct PtyWriter {
    sender: mpsc::SyncSender<PtyWriteRequest>,
    state: Arc<PtyWriteState>,
}

impl PtyWriter {
    pub fn write_bytes(
        &self,
        bytes: &[u8],
        write_context: &'static str,
        flush_context: &'static str,
    ) -> rootcause::Result<()> {
        self::queue_pty_write(self, bytes, write_context, flush_context)
    }

    pub fn write_focus_event(
        &self,
        focus_reporting: TerminalFocusReporting,
        event: TerminalFocusEvent,
    ) -> rootcause::Result<()> {
        match focus_reporting {
            TerminalFocusReporting::Disabled => Ok(()),
            TerminalFocusReporting::Enabled => {
                self.write_bytes(
                    event.bytes(),
                    "failed to write muxr terminal focus event to shell pty",
                    "failed to flush muxr terminal focus event",
                )?;
                Ok(())
            }
        }
    }

    pub fn write_terminal_replies(&self, replies: &[Vec<u8>]) -> rootcause::Result<()> {
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
            self,
            &bytes,
            "failed to write muxr terminal reply to shell pty",
            "failed to flush muxr terminal reply to shell pty",
        )?;
        Ok(())
    }

    fn enqueue(&self, mut write: PtyWrite) -> rootcause::Result<()> {
        let write_len = write.len();
        let mut queue_guard = self::lock_queue_mutex(&self.state.queue)?;
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

    pub fn shutdown(&self) -> rootcause::Result<()> {
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
                status: PtyWriterStatus::Open,
                progress_version: 0,
                queued_bytes: 0,
            }),
            queue_progress: Condvar::new(),
        }
    }

    fn close(&self) -> rootcause::Result<()> {
        let mut queue = self::lock_queue_mutex(&self.queue)?;
        queue.status.close();
        drop(queue);
        self.notify_queue_progress();
        Ok(())
    }

    fn is_closed(&self) -> rootcause::Result<bool> {
        Ok(self::lock_queue_mutex(&self.queue)?.status.is_stopped())
    }

    fn ensure_open(queue: &PtyWriteQueueState) -> rootcause::Result<()> {
        match &queue.status {
            PtyWriterStatus::Open => Ok(()),
            PtyWriterStatus::Closed => Err(report!("muxr pty writer stopped").attach("reason=pty writer is closed")),
            PtyWriterStatus::Failed(error) => Err(report!("muxr pty writer stopped").attach(error.clone())),
        }
    }

    fn record_error(&self, error: PtyWriterError) {
        match self.queue.lock() {
            Ok(mut queue) => {
                queue.status = PtyWriterStatus::Failed(error);
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
            && let PtyWriterStatus::Failed(error) = &queue.status
        {
            report = report.attach(error.clone());
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
                !queue.status.is_stopped() && queue.progress_version == observed_progress
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

// Closed and failed are both terminal states, but only failed carries the root write error for later enqueue reports.
enum PtyWriterStatus {
    Closed,
    Failed(PtyWriterError),
    Open,
}

impl PtyWriterStatus {
    fn close(&mut self) {
        if matches!(self, Self::Open) {
            *self = Self::Closed;
        }
    }

    const fn is_stopped(&self) -> bool {
        !matches!(self, Self::Open)
    }
}

struct PtyWriteQueueState {
    status: PtyWriterStatus,
    progress_version: u64,
    queued_bytes: usize,
}

const fn advance_queue_progress(queue: &mut PtyWriteQueueState) {
    queue.progress_version = queue.progress_version.wrapping_add(1);
}

pub fn spawn(mut writer: Box<dyn Write + Send>) -> (PtyWriter, thread::JoinHandle<()>) {
    let (sender, receiver) = mpsc::sync_channel(PTY_WRITE_QUEUE_LIMIT);
    let state = Arc::new(PtyWriteState::new());
    let queue = PtyWriter {
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

fn queue_pty_write(
    writer: &PtyWriter,
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
                    let error = error.into_cloneable();
                    state.record_error(error.clone());
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

fn lock_queue_mutex<T>(mutex: &Mutex<T>) -> rootcause::Result<MutexGuard<'_, T>> {
    mutex
        .lock()
        .map_err(|_| report!("poisoned muxr pty writer queue mutex"))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use muxr_core::SessionName;

    use super::*;

    #[test]
    fn test_write_terminal_replies_when_replies_exist_batches_in_order() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer();
        let written = Arc::new(Mutex::new(Vec::new()));

        queue.write_terminal_replies(&[b"one".to_vec(), b"two".to_vec()])?;
        self::drain_queued_writes(&queue, &receiver, self::capturing_pty_writer(Arc::clone(&written)))?;

        pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, b"onetwo".to_vec());
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
    fn test_pty_writer_when_limit_reached_applies_backpressure() -> rootcause::Result<()> {
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
    fn test_pty_writer_when_byte_limit_reached_applies_backpressure_until_written() -> rootcause::Result<()> {
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
    fn test_pty_writer_when_shutdown_races_full_queue_unblocks_waiting_enqueue() -> rootcause::Result<()> {
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
    fn test_pty_writer_write_bytes_when_payload_exceeds_message_limit_chunks_in_order() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer_with_limits(4, PTY_WRITE_MAX_MESSAGE_BYTES.saturating_add(1));
        let written = Arc::new(Mutex::new(Vec::new()));
        let mut payload = vec![b'a'; PTY_WRITE_MAX_MESSAGE_BYTES];
        payload.push(b'b');

        queue.write_bytes(
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
    fn test_pty_writer_shutdown_after_write_failure_preserves_error_for_later_enqueue() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer();
        queue.enqueue(PtyWrite::new(
            b"first".to_vec(),
            "failed to write first shutdown-after-error test payload",
            "failed to flush first shutdown-after-error test payload",
        ))?;

        self::run_writer_loop(&mut *self::failing_pty_writer(), &receiver, queue.state.as_ref());
        queue.shutdown()?;
        let error = queue
            .enqueue(PtyWrite::new(
                b"second".to_vec(),
                "failed to write second shutdown-after-error test payload",
                "failed to flush second shutdown-after-error test payload",
            ))
            .err()
            .ok_or_else(|| report!("expected muxr pty writer enqueue to fail after writer error and shutdown"))?;

        assert2::assert!(error.to_string().contains("test pty writer failed"));
        Ok(())
    }

    #[test]
    fn test_run_writer_loop_when_terminal_reply_write_fails_warns() -> rootcause::Result<()> {
        let session = SessionName::default();
        let (queue, receiver) = self::queued_pty_writer();
        queue.write_terminal_replies(&[b"\x1b[1;1R".to_vec()])?;
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
    fn test_write_focus_event_when_focus_reporting_is_disabled_skips_write() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer();
        let written = Arc::new(Mutex::new(Vec::new()));

        queue.write_focus_event(TerminalFocusReporting::Disabled, TerminalFocusEvent::Lost)?;
        self::drain_queued_writes(&queue, &receiver, self::capturing_pty_writer(Arc::clone(&written)))?;

        pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, Vec::<u8>::new());
        Ok(())
    }

    #[test]
    fn test_write_focus_event_when_focus_reporting_is_enabled_writes_event() -> rootcause::Result<()> {
        for (event, expected) in [
            (TerminalFocusEvent::Gained, b"\x1b[I".as_slice()),
            (TerminalFocusEvent::Lost, b"\x1b[O".as_slice()),
        ] {
            let (queue, receiver) = self::queued_pty_writer();
            let written = Arc::new(Mutex::new(Vec::new()));

            queue.write_focus_event(TerminalFocusReporting::Enabled, event)?;
            self::drain_queued_writes(&queue, &receiver, self::capturing_pty_writer(Arc::clone(&written)))?;

            pretty_assertions::assert_eq!(self::captured_pty_bytes(written.as_ref())?, expected.to_vec());
        }
        Ok(())
    }

    fn queued_pty_writer() -> (PtyWriter, mpsc::Receiver<PtyWriteRequest>) {
        self::queued_pty_writer_with_limit(PTY_WRITE_QUEUE_LIMIT)
    }

    fn queued_pty_writer_with_limit(limit: usize) -> (PtyWriter, mpsc::Receiver<PtyWriteRequest>) {
        self::queued_pty_writer_with_limits(limit, PTY_WRITE_QUEUE_BYTE_LIMIT)
    }

    fn queued_pty_writer_with_limits(
        message_limit: usize,
        byte_limit: usize,
    ) -> (PtyWriter, mpsc::Receiver<PtyWriteRequest>) {
        let (sender, receiver) = mpsc::sync_channel(message_limit);
        (
            PtyWriter {
                sender,
                state: Arc::new(PtyWriteState::with_byte_limit(byte_limit)),
            },
            receiver,
        )
    }

    fn drain_queued_writes(
        queue: &PtyWriter,
        receiver: &mpsc::Receiver<PtyWriteRequest>,
        mut writer: Box<dyn Write + Send>,
    ) -> rootcause::Result<()> {
        queue.shutdown()?;
        self::run_writer_loop(&mut *writer, receiver, queue.state.as_ref());
        Ok(())
    }

    fn capturing_pty_writer(written: Arc<Mutex<Vec<u8>>>) -> Box<dyn Write + Send> {
        Box::new(CapturingWriter { written })
    }

    fn failing_pty_writer() -> Box<dyn Write + Send> {
        Box::new(FailingWriter)
    }

    fn captured_pty_bytes(written: &Mutex<Vec<u8>>) -> rootcause::Result<Vec<u8>> {
        Ok(self::lock_queue_mutex(written)?.clone())
    }

    fn captured_flushes(flushes: &Mutex<usize>) -> rootcause::Result<usize> {
        Ok(*self::lock_queue_mutex(flushes)?)
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
