use std::time::Instant;

use muxr_core::PaneId;

use crate::client_session::ClientSessionState;
use crate::pane_tracked_process::TrackedProcessUserInteraction;
use crate::pty::PtyHandle;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneInputOutcome {
    pub cmd_handoff_pane_id: Option<PaneId>,
    pub render_dirty: bool,
    pub sync_render_deadline: bool,
}

impl PaneInputOutcome {
    const fn ignored() -> Self {
        Self {
            cmd_handoff_pane_id: None,
            render_dirty: false,
            sync_render_deadline: false,
        }
    }
}

pub fn handle_client_input(bytes: &[u8], state: &mut ClientSessionState<'_>) -> rootcause::Result<PaneInputOutcome> {
    self::handle_active_pane_bytes(
        bytes,
        state,
        crate::keyboard_input::input_interaction(bytes),
        PtyHandle::write_input,
    )
}

pub fn handle_client_paste(bytes: &[u8], state: &mut ClientSessionState<'_>) -> rootcause::Result<PaneInputOutcome> {
    // Bracketed paste can contain newlines as data; only raw input newlines mean prompt submission.
    self::handle_active_pane_bytes(
        bytes,
        state,
        TrackedProcessUserInteraction::MayEcho,
        PtyHandle::write_paste,
    )
}

pub fn handle_raw_key_bytes(bytes: &[u8], state: &mut ClientSessionState<'_>) -> rootcause::Result<PaneInputOutcome> {
    self::handle_active_pane_bytes(
        bytes,
        state,
        crate::keyboard_input::input_interaction(bytes),
        PtyHandle::write_input,
    )
}

fn handle_active_pane_bytes(
    bytes: &[u8],
    state: &mut ClientSessionState<'_>,
    interaction: TrackedProcessUserInteraction,
    write: impl FnOnce(&PtyHandle, &[u8]) -> rootcause::Result<bool>,
) -> rootcause::Result<PaneInputOutcome> {
    if bytes.is_empty() {
        return Ok(PaneInputOutcome::ignored());
    }

    self::write_active_pane_user_input(state, interaction, |handle| write(handle, bytes))
}

fn write_active_pane_user_input(
    state: &mut ClientSessionState<'_>,
    interaction: TrackedProcessUserInteraction,
    write: impl FnOnce(&PtyHandle) -> rootcause::Result<bool>,
) -> rootcause::Result<PaneInputOutcome> {
    let (pane_id, handle) = self::active_pane_handle_with_id(state)?;
    let render_dirty = write(&handle)?;
    state
        .pane_tracked_processes
        .record_user_interaction(pane_id, interaction, Instant::now());
    let cmd_handoff_pane_id =
        (interaction == TrackedProcessUserInteraction::StartsTrackedProcessWork).then_some(pane_id);
    Ok(PaneInputOutcome {
        cmd_handoff_pane_id,
        render_dirty,
        sync_render_deadline: true,
    })
}

fn active_pane_handle_with_id(state: &ClientSessionState<'_>) -> rootcause::Result<(PaneId, PtyHandle)> {
    let active_pane = state.layout.active_pane_id()?;
    let handle = state.runtimes.handle(active_pane)?;
    Ok((active_pane, handle))
}
