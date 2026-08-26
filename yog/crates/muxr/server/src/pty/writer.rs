use std::io::Write;
use std::sync::Arc;
use std::thread;

use kanal::ReceiveError;
use kanal::Receiver;
use kanal::SendError;
use kanal::Sender;
use parking_lot::Condvar;
use parking_lot::Mutex;
use parking_lot::MutexGuard;
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
    sender: Sender<PtyWriteRequest>,
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
        let mut queue_guard = self.state.queue.lock();
        loop {
            if let Err(error) = PtyWriteState::ensure_open(&queue_guard) {
                drop(queue_guard);
                return Err(error);
            }
            match self.state.reserve_write_bytes(&mut queue_guard, write_len)? {
                PtyWriteReservation::Reserved => {}
                PtyWriteReservation::WouldBlock => {
                    let observed_progress = queue_guard.progress_version;
                    queue_guard = self.state.wait_for_queue_progress(queue_guard, observed_progress);
                    continue;
                }
            }
            match self::try_send_pty_write_request(&self.sender, PtyWriteRequest::Write(write))? {
                PtyWriteSendOutcome::Sent => {
                    drop(queue_guard);
                    return Ok(());
                }
                PtyWriteSendOutcome::Full(PtyWriteRequest::Write(returned)) => {
                    write = returned;
                    PtyWriteState::release_reserved_write_bytes(&mut queue_guard, write_len);
                    let observed_progress = queue_guard.progress_version;
                    queue_guard = self.state.wait_for_queue_progress(queue_guard, observed_progress);
                }
                PtyWriteSendOutcome::Disconnected(PtyWriteRequest::Write(_)) => {
                    PtyWriteState::release_reserved_write_bytes(&mut queue_guard, write_len);
                    drop(queue_guard);
                    return Err(self.state.stopped_report("reason=pty writer channel disconnected"));
                }
                PtyWriteSendOutcome::Full(PtyWriteRequest::Shutdown)
                | PtyWriteSendOutcome::Disconnected(PtyWriteRequest::Shutdown) => {
                    drop(queue_guard);
                    return Err(report!("unexpected muxr pty writer enqueue send result"));
                }
            }
        }
    }

    pub fn shutdown(&self) -> rootcause::Result<()> {
        self.state.close();
        match self::try_send_pty_write_request(&self.sender, PtyWriteRequest::Shutdown)? {
            PtyWriteSendOutcome::Sent | PtyWriteSendOutcome::Full(PtyWriteRequest::Shutdown) => Ok(()),
            PtyWriteSendOutcome::Disconnected(PtyWriteRequest::Shutdown) => {
                Err(self.state.stopped_report("reason=pty writer channel disconnected"))
            }
            PtyWriteSendOutcome::Full(PtyWriteRequest::Write(_))
            | PtyWriteSendOutcome::Disconnected(PtyWriteRequest::Write(_)) => {
                Err(report!("unexpected muxr pty writer shutdown send result"))
            }
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

enum PtyWriteSendOutcome {
    Disconnected(PtyWriteRequest),
    Full(PtyWriteRequest),
    Sent,
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

    fn close(&self) {
        let mut queue = self.queue.lock();
        queue.status.close();
        drop(queue);
        self.notify_queue_progress();
    }

    fn stop_state(&self) -> PtyWriterStopState {
        self.queue.lock().status.stop_state()
    }

    fn ensure_open(queue: &PtyWriteQueueState) -> rootcause::Result<()> {
        match &queue.status {
            PtyWriterStatus::Open => Ok(()),
            PtyWriterStatus::Closed => Err(report!("muxr pty writer stopped").attach("reason=pty writer is closed")),
            PtyWriterStatus::Failed(error) => Err(report!("muxr pty writer stopped").attach(error.clone())),
        }
    }

    fn record_error(&self, error: PtyWriterError) {
        self.queue.lock().status = PtyWriterStatus::Failed(error);
        self.notify_queue_progress();
    }

    fn reserve_write_bytes(
        &self,
        queue: &mut PtyWriteQueueState,
        write_len: usize,
    ) -> rootcause::Result<PtyWriteReservation> {
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
            return Ok(PtyWriteReservation::WouldBlock);
        }
        queue.queued_bytes = queue
            .queued_bytes
            .checked_add(write_len)
            .ok_or_else(|| report!("muxr pty write queue byte accounting overflowed"))?;
        Ok(PtyWriteReservation::Reserved)
    }

    const fn release_reserved_write_bytes(queue: &mut PtyWriteQueueState, write_len: usize) {
        queue.queued_bytes = queue.queued_bytes.saturating_sub(write_len);
        self::advance_queue_progress(queue);
    }

    fn release_queued_bytes(&self, bytes: usize) {
        Self::release_reserved_write_bytes(&mut self.queue.lock(), bytes);
        self.queue_progress.notify_all();
    }

    fn stopped_report(&self, reason: &'static str) -> rootcause::Report {
        let mut report = report!("muxr pty writer stopped").attach(reason);
        let queue = self.queue.lock();
        if let PtyWriterStatus::Failed(error) = &queue.status {
            report = report.attach(error.clone());
        }
        drop(queue);
        report
    }

    fn wait_for_queue_progress<'a>(
        &self,
        mut guard: MutexGuard<'a, PtyWriteQueueState>,
        observed_progress: u64,
    ) -> MutexGuard<'a, PtyWriteQueueState> {
        self.queue_progress.wait_while(&mut guard, |queue| {
            queue.status.stop_state() == PtyWriterStopState::Open && queue.progress_version == observed_progress
        });
        guard
    }

    fn notify_queue_progress(&self) {
        self::advance_queue_progress(&mut self.queue.lock());
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

    const fn stop_state(&self) -> PtyWriterStopState {
        match self {
            Self::Open => PtyWriterStopState::Open,
            Self::Closed | Self::Failed(_) => PtyWriterStopState::Stopped,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PtyWriterStopState {
    Open,
    Stopped,
}

struct PtyWriteQueueState {
    status: PtyWriterStatus,
    progress_version: u64,
    queued_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PtyWriteReservation {
    Reserved,
    WouldBlock,
}

const fn advance_queue_progress(queue: &mut PtyWriteQueueState) {
    queue.progress_version = queue.progress_version.wrapping_add(1);
}

pub fn spawn(mut writer: Box<dyn Write + Send>) -> rootcause::Result<(PtyWriter, thread::JoinHandle<()>)> {
    let (sender, receiver) = kanal::bounded(PTY_WRITE_QUEUE_LIMIT);
    let state = Arc::new(PtyWriteState::new());
    let queue = PtyWriter {
        sender,
        state: Arc::clone(&state),
    };
    // Raw OS threads do not inherit thread-local tracing state, so carry both the dispatcher and span explicitly.
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let writer_handle = thread::Builder::new()
        .name("muxr-pty-writer".to_owned())
        .spawn(move || {
            tracing::dispatcher::with_default(&dispatch, || {
                let _guard = span.enter();
                self::run_writer_loop(&mut *writer, &receiver, state.as_ref());
            });
        })
        .context("failed to spawn muxr pty writer thread")?;
    Ok((queue, writer_handle))
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

fn run_writer_loop(writer: &mut dyn Write, receiver: &Receiver<PtyWriteRequest>, state: &PtyWriteState) {
    let mut batch = Vec::new();
    loop {
        let request = match state.stop_state() {
            PtyWriterStopState::Stopped => match receiver.try_recv() {
                Ok(Some(request)) => request,
                Ok(None) | Err(ReceiveError::Closed | ReceiveError::SendClosed) => break,
            },
            PtyWriterStopState::Open => match receiver.recv() {
                Ok(request) => request,
                Err(_) => break,
            },
        };
        match request {
            PtyWriteRequest::Write(write) => {
                let mut batch_bytes = write.bytes.len();
                batch.push(write);
                let drain_outcome = self::drain_pending_writes(receiver, &mut batch, &mut batch_bytes);
                let write_result = self::write_pty_batch(writer, &batch);
                state.release_queued_bytes(batch_bytes);
                if let Err(error) = write_result {
                    let error = error.into_cloneable();
                    state.record_error(error.clone());
                    crate::session::tracing::pty::writer_stopped_after_error("write_batch", &error);
                    break;
                }
                batch.clear();
                if drain_outcome == PtyWriteDrainOutcome::Shutdown {
                    break;
                }
            }
            PtyWriteRequest::Shutdown => break,
        }
    }
}

fn drain_pending_writes(
    receiver: &Receiver<PtyWriteRequest>,
    batch: &mut Vec<PtyWrite>,
    batch_bytes: &mut usize,
) -> PtyWriteDrainOutcome {
    loop {
        if batch.len() >= PTY_WRITE_BATCH_MAX_MESSAGES || *batch_bytes >= PTY_WRITE_BATCH_MAX_BYTES {
            return PtyWriteDrainOutcome::Continue;
        }
        match receiver.try_recv() {
            Ok(Some(PtyWriteRequest::Write(write))) => {
                *batch_bytes = batch_bytes.saturating_add(write.bytes.len());
                batch.push(write);
            }
            Ok(Some(PtyWriteRequest::Shutdown)) | Err(ReceiveError::Closed | ReceiveError::SendClosed) => {
                return PtyWriteDrainOutcome::Shutdown;
            }
            Ok(None) => return PtyWriteDrainOutcome::Continue,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PtyWriteDrainOutcome {
    Continue,
    Shutdown,
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

// `kanal::try_send` drops the request when a bounded queue is full. Use the option API so muxr can keep the write
// request and release byte accounting exactly like the old `SyncSender::try_send` full/disconnected error paths.
fn try_send_pty_write_request(
    sender: &Sender<PtyWriteRequest>,
    request: PtyWriteRequest,
) -> rootcause::Result<PtyWriteSendOutcome> {
    let mut pending = Some(request);
    match sender.try_send_option(&mut pending) {
        Ok(true) => Ok(PtyWriteSendOutcome::Sent),
        Ok(false) => Ok(PtyWriteSendOutcome::Full(self::pending_pty_write_request(pending)?)),
        Err(SendError::Closed | SendError::ReceiveClosed) => Ok(PtyWriteSendOutcome::Disconnected(
            self::pending_pty_write_request(pending)?,
        )),
    }
}

fn pending_pty_write_request(pending: Option<PtyWriteRequest>) -> rootcause::Result<PtyWriteRequest> {
    pending.ok_or_else(|| report!("kanal dropped muxr pty write request during failed send"))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use muxr_core::SessionName;
    use parking_lot::Mutex;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_write_terminal_replies_when_replies_exist_batches_in_order() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer();
        let written = Arc::new(Mutex::new(Vec::new()));

        queue.write_terminal_replies(&[b"one".to_vec(), b"two".to_vec()])?;
        self::drain_queued_writes(&queue, &receiver, self::capturing_pty_writer(Arc::clone(&written)))?;

        assert_that!(self::captured_pty_bytes(written.as_ref()), eq(b"onetwo".to_vec()));
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

        assert_that!(self::captured_pty_bytes(written.as_ref()), eq(b"onetwo".to_vec()));
        assert_that!(self::captured_flushes(flushes.as_ref()), eq(1));
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

        let PtyWriteSendOutcome::Full(PtyWriteRequest::Write(returned_write)) = self::try_send_pty_write_request(
            &queue.sender,
            PtyWriteRequest::Write(PtyWrite::new(
                b"two".to_vec(),
                "failed to write second bounded test payload",
                "failed to flush second bounded test payload",
            )),
        )?
        else {
            return Err(report!("expected muxr pty writer backpressure to return pending write"));
        };

        assert_that!(returned_write.bytes, eq(b"two".to_vec()));
        Ok(())
    }

    #[test]
    fn test_pty_writer_when_receiver_is_dropped_releases_reserved_bytes() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer();
        drop(receiver);

        let error = queue
            .enqueue(PtyWrite::new(
                b"lost".to_vec(),
                "failed to write dropped receiver test payload",
                "failed to flush dropped receiver test payload",
            ))
            .err()
            .ok_or_else(|| report!("expected muxr pty write enqueue to fail after receiver drop"))?;

        assert_that!(error.to_string(), contains_substring("pty writer channel disconnected"));
        assert_that!(queue.state.queue.lock().queued_bytes, eq(0));
        Ok(())
    }

    #[test]
    fn test_pty_writer_shutdown_when_receiver_is_dropped_reports_disconnected() {
        let (queue, receiver) = self::queued_pty_writer();
        drop(receiver);

        assert_that!(
            queue.shutdown(),
            err(displays_as(contains_substring("pty writer channel disconnected")))
        );
        assert_that!(queue.state.stop_state(), eq(PtyWriterStopState::Stopped));
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
        assert_that!(
            waiting_done_receiver.recv_timeout(Duration::from_millis(20)),
            err(anything())
        );

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

        assert_that!(self::captured_pty_bytes(written.as_ref()), eq(b"abcd".to_vec()));
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

        assert_that!(error, contains_substring("pty writer is closed"));
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

        assert_that!(self::captured_pty_bytes(written.as_ref()), eq(payload));
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

        assert_that!(self::captured_pty_bytes(written.as_ref()), eq(b"accepted".to_vec()));
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

        assert_that!(
            self::captured_pty_bytes(written.as_ref()),
            eq(vec![b'x'; PTY_WRITE_BATCH_MAX_MESSAGES + 1])
        );
        assert_that!(self::captured_flushes(flushes.as_ref()), eq(2));
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

        let written = self::captured_pty_bytes(written.as_ref());
        assert_that!(written.len(), eq(PTY_WRITE_BATCH_MAX_BYTES + 1));
        assert_that!(written.first(), eq(Some(&b'a')));
        assert_that!(written.last(), eq(Some(&b'c')));
        assert_that!(self::captured_flushes(flushes.as_ref()), eq(2));
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

        assert_that!(error.to_string(), contains_substring("test pty writer failed"));
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

        assert_that!(error.to_string(), contains_substring("test pty writer failed"));
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

        assert_that!(log, contains_substring("kind=\"pty_writer_stopped_after_error\""));
        assert_that!(log, contains_substring("event=\"write_batch\""));
        assert_that!(log, contains_substring("session="));
        assert_that!(log, contains_substring("test pty writer failed"));
        Ok(())
    }

    #[test]
    fn test_write_focus_event_when_focus_reporting_is_disabled_skips_write() -> rootcause::Result<()> {
        let (queue, receiver) = self::queued_pty_writer();
        let written = Arc::new(Mutex::new(Vec::new()));

        queue.write_focus_event(TerminalFocusReporting::Disabled, TerminalFocusEvent::Lost)?;
        self::drain_queued_writes(&queue, &receiver, self::capturing_pty_writer(Arc::clone(&written)))?;

        assert_that!(self::captured_pty_bytes(written.as_ref()), eq(Vec::<u8>::new()));
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

            assert_that!(self::captured_pty_bytes(written.as_ref()), eq(expected.to_vec()));
        }
        Ok(())
    }

    fn queued_pty_writer() -> (PtyWriter, Receiver<PtyWriteRequest>) {
        self::queued_pty_writer_with_limit(PTY_WRITE_QUEUE_LIMIT)
    }

    fn queued_pty_writer_with_limit(limit: usize) -> (PtyWriter, Receiver<PtyWriteRequest>) {
        self::queued_pty_writer_with_limits(limit, PTY_WRITE_QUEUE_BYTE_LIMIT)
    }

    fn queued_pty_writer_with_limits(
        message_limit: usize,
        byte_limit: usize,
    ) -> (PtyWriter, Receiver<PtyWriteRequest>) {
        let (sender, receiver) = kanal::bounded(message_limit);
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
        receiver: &Receiver<PtyWriteRequest>,
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

    fn captured_pty_bytes(written: &Mutex<Vec<u8>>) -> Vec<u8> {
        written.lock().clone()
    }

    fn captured_flushes(flushes: &Mutex<usize>) -> usize {
        *flushes.lock()
    }

    struct CapturingWriter {
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for CapturingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut written = self.written.lock();
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
            let mut written = self.written.lock();
            written.extend_from_slice(buf);
            drop(written);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            let mut flushes = self.flushes.lock();
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
