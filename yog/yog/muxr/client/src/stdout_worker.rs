use std::collections::VecDeque;
use std::io::Write;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::thread;

use rootcause::report;

const MANDATORY_TRANSACTION_LIMIT: usize = 128;

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
    output: VecDeque<OutputCommand>,
    mandatory_count: usize,
}

enum OutputCommand {
    Mandatory(Vec<u8>),
    Render(Vec<u8>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplaceRender {
    Queued,
    Replaced,
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
        state.output.push_back(OutputCommand::Mandatory(transaction));
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
        full_redraw: impl FnOnce() -> rootcause::Result<Option<Vec<u8>>>,
    ) -> rootcause::Result<ReplaceRender> {
        let mut state = self::lock_state(&self.shared)?;
        ensure_open(&state)?;
        let State {
            output,
            mandatory_count,
            ..
        } = &mut *state;
        let replaced = self::replace_render_command(output, mandatory_count, transaction, full_redraw)?;
        drop(state);
        self.shared.wake.notify_one();
        Ok(if replaced {
            ReplaceRender::Replaced
        } else {
            ReplaceRender::Queued
        })
    }
}

fn replace_render_command(
    output: &mut VecDeque<OutputCommand>,
    mandatory_count: &mut usize,
    transaction: Vec<u8>,
    full_redraw: impl FnOnce() -> rootcause::Result<Option<Vec<u8>>>,
) -> rootcause::Result<bool> {
    if let Some(position) = output
        .iter_mut()
        .position(|command| matches!(command, OutputCommand::Render(_)))
    {
        let full_redraw = full_redraw()?
            .ok_or_else(|| report!("muxr client cannot supersede a pending render without a full redraw"))?;
        let superseded_mandatory = output
            .iter()
            .skip(position.saturating_add(1))
            .filter(|command| matches!(command, OutputCommand::Mandatory(_)))
            .count();
        *mandatory_count = mandatory_count
            .checked_sub(superseded_mandatory)
            .ok_or_else(|| report!("muxr client stdout mandatory queue count underflowed"))?;
        output.truncate(position);
        output.push_back(OutputCommand::Render(full_redraw));
        Ok(true)
    } else {
        output.push_back(OutputCommand::Render(transaction));
        Ok(false)
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
        let transaction = {
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
            let command = state.output.pop_front();
            if matches!(command, Some(OutputCommand::Mandatory(_))) {
                state.mandatory_count = state.mandatory_count.saturating_sub(1);
            }
            command.map(|command| match command {
                OutputCommand::Mandatory(transaction) | OutputCommand::Render(transaction) => transaction,
            })
        };
        let Some(transaction) = transaction else {
            continue;
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
                OutputCommand::Render(b"render".to_vec()),
                OutputCommand::Mandatory(b"selection".to_vec()),
            ]),
            mandatory_count: 1,
        };

        let first = state.output.pop_front();
        let second = state.output.pop_front();
        assert!(matches!(first, Some(OutputCommand::Render(transaction)) if transaction == b"render"));
        assert!(matches!(second, Some(OutputCommand::Mandatory(transaction)) if transaction == b"selection"));
    }

    #[test]
    fn test_replace_render_compacts_intervening_state_into_full_redraw() -> rootcause::Result<()> {
        let mut output = VecDeque::from([
            OutputCommand::Render(b"old-render".to_vec()),
            OutputCommand::Mandatory(b"selection".to_vec()),
        ]);
        let mut mandatory_count = 1;

        assert_that!(
            replace_render_command(&mut output, &mut mandatory_count, b"incremental".to_vec(), || {
                Ok(Some(b"full-redraw".to_vec()))
            },)?,
            eq(true)
        );
        assert_that!(
            matches!(output.front(), Some(OutputCommand::Render(transaction)) if transaction == b"full-redraw"),
            eq(true)
        );
        assert_that!(output.len(), eq(1));
        assert_that!(mandatory_count, eq(0));
        Ok(())
    }

    #[test]
    fn test_replace_render_without_pending_render_queues_incremental_without_generating_full_redraw()
    -> rootcause::Result<()> {
        let mut output = VecDeque::new();
        let mut mandatory_count: usize = 0;

        assert_that!(
            replace_render_command(&mut output, &mut mandatory_count, b"incremental".to_vec(), || {
                Err(report!("full redraw should not be generated"))
            },)?,
            eq(false)
        );
        assert_that!(
            matches!(output.front(), Some(OutputCommand::Render(transaction)) if transaction == b"incremental"),
            eq(true)
        );
        assert_that!(mandatory_count, eq(0));
        Ok(())
    }

    #[test]
    fn test_repeated_render_state_supersession_keeps_one_payload() -> rootcause::Result<()> {
        let mut output = VecDeque::from([OutputCommand::Render(b"render-0".to_vec())]);
        let mut mandatory_count: usize = 0;

        for generation in 1..=MANDATORY_TRANSACTION_LIMIT {
            output.push_back(OutputCommand::Mandatory(format!("selection-{generation}").into_bytes()));
            mandatory_count = mandatory_count.saturating_add(1);
            assert_that!(
                replace_render_command(
                    &mut output,
                    &mut mandatory_count,
                    format!("incremental-{generation}").into_bytes(),
                    || { Ok(Some(format!("full-redraw-{generation}").into_bytes())) },
                )?,
                eq(true)
            );
            assert_that!(output.len(), eq(1));
            assert_that!(mandatory_count, eq(0));
        }
        assert_that!(
            matches!(output.front(), Some(OutputCommand::Render(transaction)) if transaction == b"full-redraw-128"),
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
                output: VecDeque::from([OutputCommand::Render(b"old-render".to_vec())]),
                mandatory_count: 0,
            }),
            wake: Condvar::new(),
        });
        let sender = StdoutSender {
            shared: Arc::clone(&shared),
        };

        let replaced = sender.replace_render(b"incremental".to_vec(), || {
            if shared.state.try_lock().is_ok() {
                return Err(report!("stdout queue lock was released before full redraw generation"));
            }
            Ok(Some(b"full-redraw".to_vec()))
        })?;
        assert_that!(replaced, eq(ReplaceRender::Replaced));
        let state = self::lock_state(&shared)?;
        let installed =
            matches!(state.output.front(), Some(OutputCommand::Render(transaction)) if transaction == b"full-redraw");
        drop(state);
        assert_that!(installed, eq(true));
        Ok(())
    }
}
