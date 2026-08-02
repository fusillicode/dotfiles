use std::collections::VecDeque;
use std::io::Write;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::thread;

use rootcause::report;

const MANDATORY_TRANSACTION_LIMIT: usize = 128;
const QUEUED_TRANSACTION_BYTE_LIMIT: usize = 4 * 1024 * 1024;

/// A single stdout owner with ordered mandatory writes and one replaceable render slot.
pub struct StdoutWorker {
    shared: Arc<Shared>,
    handle: Option<thread::JoinHandle<()>>,
}

#[derive(Clone)]
pub struct StdoutSender {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

struct State {
    closed: bool,
    failed: Option<String>,
    output: VecDeque<OutputCmd>,
    mandatory_count: usize,
    queued_bytes: usize,
}

enum OutputCmd {
    Mandatory(Vec<u8>),
    Render(Vec<u8>),
}

impl StdoutWorker {
    pub fn spawn() -> (StdoutSender, Self, tokio::sync::oneshot::Receiver<String>) {
        let (failure_sender, failure_receiver) = tokio::sync::oneshot::channel();
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                closed: false,
                failed: None,
                output: VecDeque::new(),
                mandatory_count: 0,
                queued_bytes: 0,
            }),
            wake: Condvar::new(),
        });
        let worker_shared = Arc::clone(&shared);
        let handle = thread::spawn(move || run(&worker_shared, failure_sender));
        (
            StdoutSender {
                shared: Arc::clone(&shared),
            },
            Self {
                shared,
                handle: Some(handle),
            },
            failure_receiver,
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
    pub fn send_mandatory(&self, transaction: Vec<u8>) -> rootcause::Result<()> {
        if transaction.is_empty() {
            return Ok(());
        }
        let mut state = self::lock_state(&self.shared)?;
        ensure_open(&state)?;
        if state.mandatory_count == MANDATORY_TRANSACTION_LIMIT {
            return Err(report!("muxr client stdout mandatory queue is full"));
        }
        self::reserve_queued_bytes(&state, transaction.len())?;
        state.queued_bytes = state
            .queued_bytes
            .checked_add(transaction.len())
            .ok_or_else(|| report!("muxr client stdout queued byte count overflowed"))?;
        state.output.push_back(OutputCmd::Mandatory(transaction));
        state.mandatory_count = state
            .mandatory_count
            .checked_add(1)
            .ok_or_else(|| report!("muxr client stdout mandatory queue count overflowed"))?;
        drop(state);
        self.shared.wake.notify_one();
        Ok(())
    }

    pub fn replace_render(
        &self,
        transaction: Vec<u8>,
        replacement_transaction: impl FnOnce() -> rootcause::Result<Option<Vec<u8>>>,
    ) -> rootcause::Result<()> {
        let mut state = self::lock_state(&self.shared)?;
        ensure_open(&state)?;
        self::replace_render_cmd(&mut state, transaction, replacement_transaction)?;
        drop(state);
        self.shared.wake.notify_one();
        Ok(())
    }
}

fn replace_render_cmd(
    state: &mut State,
    transaction: Vec<u8>,
    replacement_transaction: impl FnOnce() -> rootcause::Result<Option<Vec<u8>>>,
) -> rootcause::Result<bool> {
    if let Some(position) = state.output.iter().position(|cmd| matches!(cmd, OutputCmd::Render(_))) {
        let replacement_transaction = replacement_transaction()?
            .ok_or_else(|| report!("muxr client cannot supersede a pending render without a full redraw"))?;
        let retained_bytes = state
            .output
            .iter()
            .take(position)
            .map(OutputCmd::len)
            .try_fold(replacement_transaction.len(), usize::checked_add)
            .ok_or_else(|| report!("muxr client stdout queued byte count overflowed"))?;
        if retained_bytes > QUEUED_TRANSACTION_BYTE_LIMIT {
            return Err(report!("muxr client stdout queued byte budget is exhausted"));
        }
        state.output.truncate(position);
        state.mandatory_count = state
            .output
            .iter()
            .filter(|cmd| matches!(cmd, OutputCmd::Mandatory(_)))
            .count();
        state.queued_bytes = retained_bytes;
        state.output.push_back(OutputCmd::Render(replacement_transaction));
        Ok(true)
    } else {
        self::reserve_queued_bytes(state, transaction.len())?;
        state.queued_bytes = state
            .queued_bytes
            .checked_add(transaction.len())
            .ok_or_else(|| report!("muxr client stdout queued byte count overflowed"))?;
        state.output.push_back(OutputCmd::Render(transaction));
        Ok(false)
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
            Self::Mandatory(transaction) | Self::Render(transaction) => transaction.len(),
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

fn run(shared: &Shared, failure_sender: tokio::sync::oneshot::Sender<String>) {
    let mut stdout = std::io::stdout();
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
            if matches!(cmd, Some(OutputCmd::Mandatory(_))) {
                state.mandatory_count = state.mandatory_count.saturating_sub(1);
            }
            cmd
        };
        let Some(cmd) = cmd else {
            continue;
        };
        let transaction = match cmd {
            OutputCmd::Mandatory(transaction) | OutputCmd::Render(transaction) => transaction,
        };
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
    }
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_output_queue_preserves_render_before_later_mandatory_transaction() {
        let mut state = State {
            closed: false,
            failed: None,
            output: VecDeque::from([
                OutputCmd::Render(b"render".to_vec()),
                OutputCmd::Mandatory(b"selection".to_vec()),
            ]),
            mandatory_count: 1,
            queued_bytes: b"render".len() + b"selection".len(),
        };

        let first = state.output.pop_front();
        let second = state.output.pop_front();
        assert!(matches!(first, Some(OutputCmd::Render(transaction)) if transaction == b"render"));
        assert!(matches!(second, Some(OutputCmd::Mandatory(transaction)) if transaction == b"selection"));
    }

    #[test]
    fn test_replace_render_compacts_intervening_state_into_full_redraw() -> rootcause::Result<()> {
        let mut state = State {
            closed: false,
            failed: None,
            output: VecDeque::from([
                OutputCmd::Render(b"old-render".to_vec()),
                OutputCmd::Mandatory(b"selection".to_vec()),
            ]),
            mandatory_count: 1,
            queued_bytes: b"old-render".len() + b"selection".len(),
        };

        assert_that!(
            replace_render_cmd(&mut state, b"incremental".to_vec(), || {
                Ok(Some(b"full-redraw".to_vec()))
            },)?,
            eq(true)
        );
        assert_that!(
            matches!(state.output.front(), Some(OutputCmd::Render(transaction)) if transaction == b"full-redraw"),
            eq(true)
        );
        assert_that!(state.output.len(), eq(1));
        assert_that!(state.mandatory_count, eq(0));
        assert_that!(state.queued_bytes, eq(b"full-redraw".len()));
        Ok(())
    }

    #[test]
    fn test_replace_render_without_pending_render_queues_incremental_without_generating_full_redraw()
    -> rootcause::Result<()> {
        let mut state = State {
            closed: false,
            failed: None,
            output: VecDeque::new(),
            mandatory_count: 0,
            queued_bytes: 0,
        };

        assert_that!(
            replace_render_cmd(&mut state, b"incremental".to_vec(), || {
                Err(report!("full redraw should not be generated"))
            },)?,
            eq(false)
        );
        assert_that!(
            matches!(state.output.front(), Some(OutputCmd::Render(transaction)) if transaction == b"incremental"),
            eq(true)
        );
        assert_that!(state.mandatory_count, eq(0));
        Ok(())
    }

    #[test]
    fn test_repeated_render_state_supersession_keeps_one_payload() -> rootcause::Result<()> {
        let mut state = State {
            closed: false,
            failed: None,
            output: VecDeque::from([OutputCmd::Render(b"render-0".to_vec())]),
            mandatory_count: 0,
            queued_bytes: b"render-0".len(),
        };

        for generation in 1..=MANDATORY_TRANSACTION_LIMIT {
            let generation = u64::try_from(generation)
                .map_err(|_| report!("muxr stdout test render generation does not fit in u64"))?;
            let selection = format!("selection-{generation}").into_bytes();
            state.queued_bytes += selection.len();
            state.output.push_back(OutputCmd::Mandatory(selection));
            state.mandatory_count = state.mandatory_count.saturating_add(1);
            assert_that!(
                replace_render_cmd(&mut state, format!("incremental-{generation}").into_bytes(), || {
                    Ok(Some(format!("full-redraw-{generation}").into_bytes()))
                },)?,
                eq(true)
            );
            assert_that!(state.output.len(), eq(1));
            assert_that!(state.mandatory_count, eq(0));
        }
        assert_that!(
            matches!(state.output.front(), Some(OutputCmd::Render(transaction)) if transaction == b"full-redraw-128"),
            eq(true)
        );
        Ok(())
    }

    #[test]
    fn test_sender_keeps_queue_locked_until_full_redraw_replacement_is_installed() -> rootcause::Result<()> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                closed: false,
                failed: None,
                output: VecDeque::from([OutputCmd::Render(b"old-render".to_vec())]),
                mandatory_count: 0,
                queued_bytes: b"old-render".len(),
            }),
            wake: Condvar::new(),
        });
        let sender = StdoutSender {
            shared: Arc::clone(&shared),
        };

        sender.replace_render(b"incremental".to_vec(), || {
            if shared.state.try_lock().is_ok() {
                return Err(report!("stdout queue lock was released before full redraw generation"));
            }
            Ok(Some(b"full-redraw".to_vec()))
        })?;
        let state = self::lock_state(&shared)?;
        let installed =
            matches!(state.output.front(), Some(OutputCmd::Render(transaction)) if transaction == b"full-redraw");
        drop(state);
        assert_that!(installed, eq(true));
        Ok(())
    }

    #[test]
    fn test_queued_byte_budget_rejects_another_transaction() {
        let state = State {
            closed: false,
            failed: None,
            output: VecDeque::new(),
            mandatory_count: 0,
            queued_bytes: QUEUED_TRANSACTION_BYTE_LIMIT,
        };

        assert_that!(reserve_queued_bytes(&state, 1).is_err(), eq(true));
    }
}
