use std::time::Instant;

use muxr_core::ClientKey;
use muxr_core::PaneId;

use crate::client::session::ClientSessionState;
use crate::pane::tracked_process::TrackedProcessClientChange;
use crate::pane::tracked_process::TrackedProcessUserInteraction;
use crate::pty::PtyHandle;
use crate::pty::PtyViewportMove;
use crate::render_state::PaneInputRenderPriority;
use crate::render_state::PaneRenderSignal;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneInputOutcome {
    pub cmd_handoff_pane_id: Option<PaneId>,
    pub render_priority: PaneInputRenderPriority,
    pub render_signal: PaneRenderSignal,
    pub tracked_process_change: Option<TrackedProcessClientChange>,
}

impl PaneInputOutcome {
    const fn ignored() -> Self {
        Self {
            cmd_handoff_pane_id: None,
            render_priority: PaneInputRenderPriority::Bulk,
            render_signal: PaneRenderSignal::Unchanged,
            tracked_process_change: None,
        }
    }
}

pub fn handle_client_input(bytes: &[u8], state: &mut ClientSessionState<'_>) -> rootcause::Result<PaneInputOutcome> {
    self::handle_active_pane_bytes(
        bytes,
        state,
        TrackedProcessUserInteraction::from(bytes),
        PaneInputRenderPriority::Interactive,
        PtyHandle::write_input,
    )
}

pub fn handle_client_paste(bytes: &[u8], state: &mut ClientSessionState<'_>) -> rootcause::Result<PaneInputOutcome> {
    // Bracketed paste can contain newlines as data; only raw input newlines mean prompt submission.
    self::handle_active_pane_bytes(
        bytes,
        state,
        TrackedProcessUserInteraction::MayEcho,
        PaneInputRenderPriority::Bulk,
        PtyHandle::write_paste,
    )
}

pub fn handle_client_key(key: &ClientKey, state: &mut ClientSessionState<'_>) -> rootcause::Result<PaneInputOutcome> {
    let (pane_id, handle) = self::active_pane_handle_with_id(state)?;
    let keyboard_protocol = handle.application_mode().keyboard_protocol;
    let Some(bytes) = crate::keyboard_input::pane_key_input_bytes(key, keyboard_protocol) else {
        return Ok(PaneInputOutcome::ignored());
    };
    if bytes.is_empty() {
        return Ok(PaneInputOutcome::ignored());
    }

    let interaction = TrackedProcessUserInteraction::from_key_input(key, &bytes);
    let viewport_move = handle.write_input(&bytes)?;
    let tracked_process_change =
        state
            .pane_tracked_processes
            .record_focused_client_user_interaction(pane_id, interaction, Instant::now());
    let cmd_handoff_pane_id =
        (interaction == TrackedProcessUserInteraction::StartsTrackedProcessWork).then_some(pane_id);
    Ok(PaneInputOutcome {
        cmd_handoff_pane_id,
        render_priority: PaneInputRenderPriority::Interactive,
        render_signal: PaneRenderSignal::from_dmg_and_deadline(
            if viewport_move == crate::pty::PtyViewportMove::MovedToBottom {
                crate::render_state::ClientRenderDmg::Dirty
            } else {
                crate::render_state::ClientRenderDmg::Clean
            },
            crate::render_state::PaneRenderDeadlineSync::Sync,
        ),
        tracked_process_change,
    })
}

fn handle_active_pane_bytes(
    bytes: &[u8],
    state: &mut ClientSessionState<'_>,
    interaction: TrackedProcessUserInteraction,
    render_priority: PaneInputRenderPriority,
    write: impl FnOnce(&PtyHandle, &[u8]) -> rootcause::Result<PtyViewportMove>,
) -> rootcause::Result<PaneInputOutcome> {
    if bytes.is_empty() {
        return Ok(PaneInputOutcome::ignored());
    }

    self::write_active_pane_user_input(state, interaction, render_priority, |handle| write(handle, bytes))
}

fn write_active_pane_user_input(
    state: &mut ClientSessionState<'_>,
    interaction: TrackedProcessUserInteraction,
    render_priority: PaneInputRenderPriority,
    write: impl FnOnce(&PtyHandle) -> rootcause::Result<PtyViewportMove>,
) -> rootcause::Result<PaneInputOutcome> {
    let (pane_id, handle) = self::active_pane_handle_with_id(state)?;
    let viewport_move = write(&handle)?;
    let tracked_process_change =
        state
            .pane_tracked_processes
            .record_focused_client_user_interaction(pane_id, interaction, Instant::now());
    let cmd_handoff_pane_id =
        (interaction == TrackedProcessUserInteraction::StartsTrackedProcessWork).then_some(pane_id);
    Ok(PaneInputOutcome {
        cmd_handoff_pane_id,
        render_priority,
        render_signal: PaneRenderSignal::from_dmg_and_deadline(
            if viewport_move == crate::pty::PtyViewportMove::MovedToBottom {
                crate::render_state::ClientRenderDmg::Dirty
            } else {
                crate::render_state::ClientRenderDmg::Clean
            },
            crate::render_state::PaneRenderDeadlineSync::Sync,
        ),
        tracked_process_change,
    })
}

fn active_pane_handle_with_id(state: &ClientSessionState<'_>) -> rootcause::Result<(PaneId, PtyHandle)> {
    // The pane id is a same-turn focused-input proof. Input handlers consume it synchronously before any await or
    // layout mutation, so tracked-process recording does not need another active-pane lookup on the per-key path.
    let pane_id = state.layout.active_pane_id()?;
    let handle = state.runtimes.handle(pane_id)?;
    Ok((pane_id, handle))
}
