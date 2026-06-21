use std::time::Instant;

use muxr_core::ClientKey;
use muxr_core::PaneId;

use crate::client::session::ClientSessionState;
use crate::pane::tracked_process::TrackedProcessClientChange;
use crate::pane::tracked_process::TrackedProcessUserInteraction;
use crate::pty::PtyHandle;
use crate::state::ActivePaneId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneInputOutcome {
    pub cmd_handoff_pane_id: Option<PaneId>,
    pub interactive_render: bool,
    pub render_dirty: bool,
    pub sync_render_deadline: bool,
    pub tracked_process_change: Option<TrackedProcessClientChange>,
}

impl PaneInputOutcome {
    const fn ignored() -> Self {
        Self {
            cmd_handoff_pane_id: None,
            interactive_render: false,
            render_dirty: false,
            sync_render_deadline: false,
            tracked_process_change: None,
        }
    }
}

pub fn handle_client_input(bytes: &[u8], state: &mut ClientSessionState<'_>) -> rootcause::Result<PaneInputOutcome> {
    self::handle_active_pane_bytes(
        bytes,
        state,
        crate::keyboard_input::input_interaction(bytes),
        true,
        PtyHandle::write_input,
    )
}

pub fn handle_client_paste(bytes: &[u8], state: &mut ClientSessionState<'_>) -> rootcause::Result<PaneInputOutcome> {
    // Bracketed paste can contain newlines as data; only raw input newlines mean prompt submission.
    self::handle_active_pane_bytes(
        bytes,
        state,
        TrackedProcessUserInteraction::MayEcho,
        false,
        PtyHandle::write_paste,
    )
}

pub fn handle_client_key(key: &ClientKey, state: &mut ClientSessionState<'_>) -> rootcause::Result<PaneInputOutcome> {
    let (active_pane, handle) = self::active_pane_handle_with_id(state)?;
    let keyboard_protocol = handle.application_mode()?.keyboard_protocol;
    let Some(bytes) = crate::keyboard_input::pane_key_input_bytes(key, keyboard_protocol) else {
        return Ok(PaneInputOutcome::ignored());
    };
    if bytes.is_empty() {
        return Ok(PaneInputOutcome::ignored());
    }

    let interaction = crate::keyboard_input::key_input_interaction(key, &bytes);
    let render_dirty = handle.write_input(&bytes)?;
    let pane_id = active_pane.pane_id();
    let tracked_process_change =
        state
            .pane_tracked_processes
            .record_active_pane_user_interaction(active_pane, interaction, Instant::now());
    let cmd_handoff_pane_id =
        (interaction == TrackedProcessUserInteraction::StartsTrackedProcessWork).then_some(pane_id);
    Ok(PaneInputOutcome {
        cmd_handoff_pane_id,
        interactive_render: true,
        render_dirty,
        sync_render_deadline: true,
        tracked_process_change,
    })
}

fn handle_active_pane_bytes(
    bytes: &[u8],
    state: &mut ClientSessionState<'_>,
    interaction: TrackedProcessUserInteraction,
    interactive_render: bool,
    write: impl FnOnce(&PtyHandle, &[u8]) -> rootcause::Result<bool>,
) -> rootcause::Result<PaneInputOutcome> {
    if bytes.is_empty() {
        return Ok(PaneInputOutcome::ignored());
    }

    self::write_active_pane_user_input(state, interaction, interactive_render, |handle| write(handle, bytes))
}

fn write_active_pane_user_input(
    state: &mut ClientSessionState<'_>,
    interaction: TrackedProcessUserInteraction,
    interactive_render: bool,
    write: impl FnOnce(&PtyHandle) -> rootcause::Result<bool>,
) -> rootcause::Result<PaneInputOutcome> {
    let (active_pane, handle) = self::active_pane_handle_with_id(state)?;
    let render_dirty = write(&handle)?;
    let pane_id = active_pane.pane_id();
    let tracked_process_change =
        state
            .pane_tracked_processes
            .record_active_pane_user_interaction(active_pane, interaction, Instant::now());
    let cmd_handoff_pane_id =
        (interaction == TrackedProcessUserInteraction::StartsTrackedProcessWork).then_some(pane_id);
    Ok(PaneInputOutcome {
        cmd_handoff_pane_id,
        interactive_render,
        render_dirty,
        sync_render_deadline: true,
        tracked_process_change,
    })
}

fn active_pane_handle_with_id(state: &ClientSessionState<'_>) -> rootcause::Result<(ActivePaneId, PtyHandle)> {
    let active_pane = state.layout.active_pane_token()?;
    let handle = state.runtimes.handle(active_pane.pane_id())?;
    Ok((active_pane, handle))
}
