use std::collections::VecDeque;
use std::io::Write;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::thread;

use rootcause::report;

const QUEUED_TRANSACTION_BYTE_LIMIT: usize = 4 * 1024 * 1024;

/// A single stdout owner that reports each successful render flush.
pub struct StdoutWorker {
    shared: Arc<Shared>,
    handle: Option<thread::JoinHandle<()>>,
}

#[derive(Clone)]
pub struct StdoutSender {
    shared: Arc<Shared>,
}

struct Shared {
    completed: tokio::sync::mpsc::UnboundedSender<()>,
    state: Mutex<State>,
    wake: Condvar,
}

struct State {
    closed: bool,
    failed: Option<String>,
    output: VecDeque<OutputCmd>,
    queued_bytes: usize,
}

enum OutputCmd {
    Render(Vec<u8>),
}

impl StdoutWorker {
    pub fn spawn() -> (
        StdoutSender,
        Self,
        tokio::sync::oneshot::Receiver<String>,
        tokio::sync::mpsc::UnboundedReceiver<()>,
    ) {
        let (failure_sender, failure_receiver) = tokio::sync::oneshot::channel();
        let (completed_sender, completed_receiver) = tokio::sync::mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            completed: completed_sender,
            state: Mutex::new(State {
                closed: false,
                failed: None,
                output: VecDeque::new(),
                queued_bytes: 0,
            }),
            wake: Condvar::new(),
        });
        let worker_shared = Arc::clone(&shared);
        let handle = thread::spawn(move || {
            let mut stdout = std::io::stdout();
            run(&worker_shared, &mut stdout, failure_sender);
        });
        (
            StdoutSender {
                shared: Arc::clone(&shared),
            },
            Self {
                shared,
                handle: Some(handle),
            },
            failure_receiver,
            completed_receiver,
        )
    }
}

impl Drop for StdoutWorker {
    fn drop(&mut self) {
        if let Ok(mut state) = self::lock_state(&self.shared) {
            state.closed = true;
            drop(state);
            self.shared.wake.notify_all();
        }
        if let Some(handle) = self.handle.take() {
            let _joined = handle.join();
        }
    }
}

impl StdoutSender {
    pub fn send_render(&self, transaction: Vec<u8>) -> rootcause::Result<()> {
        if transaction.is_empty() {
            return Ok(());
        }
        let mut state = self::lock_state(&self.shared)?;
        ensure_open(&state)?;
        self::reserve_queued_bytes(&state, transaction.len())?;
        state.queued_bytes = state
            .queued_bytes
            .checked_add(transaction.len())
            .ok_or_else(|| report!("muxr client stdout queued byte count overflowed"))?;
        state.output.push_back(OutputCmd::Render(transaction));
        drop(state);
        self.shared.wake.notify_one();
        Ok(())
    }
}

fn reserve_queued_bytes(state: &State, additional_bytes: usize) -> rootcause::Result<()> {
    let next = state
        .queued_bytes
        .checked_add(additional_bytes)
        .ok_or_else(|| report!("muxr client stdout queued byte count overflowed"))?;
    if next > QUEUED_TRANSACTION_BYTE_LIMIT {
        return Err(report!("muxr client stdout queued byte budget is exhausted"));
    }
    Ok(())
}

impl OutputCmd {
    const fn len(&self) -> usize {
        match self {
            Self::Render(transaction) => transaction.len(),
        }
    }
}

fn ensure_open(state: &State) -> rootcause::Result<()> {
    if let Some(error) = &state.failed {
        return Err(report!("muxr client stdout worker failed").attach(error.clone()));
    }
    if state.closed {
        return Err(report!("muxr client stdout worker is closed"));
    }
    Ok(())
}

fn lock_state(shared: &Shared) -> rootcause::Result<MutexGuard<'_, State>> {
    shared
        .state
        .lock()
        .map_err(|_| report!("muxr stdout worker state poisoned"))
}

fn run(shared: &Shared, stdout: &mut impl Write, failure_sender: tokio::sync::oneshot::Sender<String>) {
    loop {
        let cmd = {
            let Ok(mut state) = self::lock_state(shared) else {
                return;
            };
            while !state.closed && state.output.is_empty() {
                let Ok(next_state) = shared.wake.wait(state) else {
                    return;
                };
                state = next_state;
            }
            if state.closed && state.output.is_empty() {
                return;
            }
            let cmd = state.output.pop_front();
            if let Some(cmd) = cmd.as_ref() {
                state.queued_bytes = state.queued_bytes.saturating_sub(cmd.len());
            }
            cmd
        };
        let Some(cmd) = cmd else {
            continue;
        };
        let OutputCmd::Render(transaction) = cmd;
        if let Err(error) = stdout.write_all(&transaction).and_then(|()| stdout.flush()) {
            if let Ok(mut state) = self::lock_state(shared) {
                state.failed = Some(error.to_string());
                state.closed = true;
                drop(state);
            }
            shared.wake.notify_all();
            let _sent = failure_sender.send(error.to_string());
            return;
        }
        let _sent = shared.completed.send(());
    }
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_queued_byte_budget_rejects_another_transaction() {
        let state = State {
            closed: false,
            failed: None,
            output: VecDeque::new(),
            queued_bytes: QUEUED_TRANSACTION_BYTE_LIMIT,
        };

        assert_that!(reserve_queued_bytes(&state, 1).is_err(), eq(true));
    }

    #[test]
    fn test_run_when_render_flushes_sends_completion_after_output() {
        let (completed, mut completed_receiver) = tokio::sync::mpsc::unbounded_channel();
        let shared = Shared {
            completed,
            state: Mutex::new(State {
                closed: true,
                failed: None,
                output: VecDeque::from([OutputCmd::Render(b"render".to_vec())]),
                queued_bytes: b"render".len(),
            }),
            wake: Condvar::new(),
        };
        let (failure_sender, mut failure_receiver) = tokio::sync::oneshot::channel();
        let mut output = Vec::new();

        run(&shared, &mut output, failure_sender);

        assert_that!(output, eq(b"render".to_vec()));
        assert_that!(completed_receiver.try_recv(), eq(Ok(())));
        assert_that!(failure_receiver.try_recv().is_err(), eq(true));
    }

    #[test]
    fn test_run_when_render_flush_blocks_completion_until_flush_finishes() -> rootcause::Result<()> {
        let (completed, mut completed_receiver) = tokio::sync::mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            completed,
            state: Mutex::new(State {
                closed: true,
                failed: None,
                output: VecDeque::from([OutputCmd::Render(b"render".to_vec())]),
                queued_bytes: b"render".len(),
            }),
            wake: Condvar::new(),
        });
        let (flush_started_sender, flush_started_receiver) = std::sync::mpsc::channel();
        let (flush_release_sender, flush_release_receiver) = std::sync::mpsc::channel();
        let (failure_sender, _failure_receiver) = tokio::sync::oneshot::channel();
        let worker_shared = Arc::clone(&shared);
        let handle = thread::spawn(move || {
            let mut output = BlockingWriter {
                flush_release_receiver,
                flush_started_sender,
            };
            run(&worker_shared, &mut output, failure_sender);
        });

        flush_started_receiver.recv()?;
        assert_that!(
            completed_receiver.try_recv(),
            err(matches_pattern!(tokio::sync::mpsc::error::TryRecvError::Empty))
        );
        flush_release_sender.send(())?;
        handle
            .join()
            .map_err(|_| report!("muxr stdout blocking-writer test thread panicked"))?;

        assert_that!(completed_receiver.try_recv(), eq(Ok(())));
        Ok(())
    }

    #[test]
    fn test_run_when_render_write_fails_marks_worker_failed_without_completion() -> rootcause::Result<()> {
        let (completed, mut completed_receiver) = tokio::sync::mpsc::unbounded_channel();
        let shared = Shared {
            completed,
            state: Mutex::new(State {
                closed: true,
                failed: None,
                output: VecDeque::from([OutputCmd::Render(b"render".to_vec())]),
                queued_bytes: b"render".len(),
            }),
            wake: Condvar::new(),
        };
        let (failure_sender, mut failure_receiver) = tokio::sync::oneshot::channel();

        run(&shared, &mut FailingWriter, failure_sender);

        assert_that!(failure_receiver.try_recv().is_ok(), eq(true));
        assert_that!(
            completed_receiver.try_recv(),
            err(matches_pattern!(tokio::sync::mpsc::error::TryRecvError::Empty))
        );
        let state = lock_state(&shared)?;
        assert_that!(state.failed.is_some(), eq(true));
        assert_that!(state.closed, eq(true));
        drop(state);
        Ok(())
    }

    struct BlockingWriter {
        flush_release_receiver: std::sync::mpsc::Receiver<()>,
        flush_started_sender: std::sync::mpsc::Sender<()>,
    }

    impl Write for BlockingWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flush_started_sender
                .send(())
                .map_err(|_| std::io::Error::other("muxr stdout flush observer disconnected"))?;
            self.flush_release_receiver
                .recv()
                .map_err(|_| std::io::Error::other("muxr stdout flush release disconnected"))
        }
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("injected muxr stdout write failure"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
