use std::time::Instant;

use muxr_transport::ServerEventWriter;

use crate::client::session::ClientSessionState;
use crate::client::session::ReapedPanes;
use crate::client::timers::ClientTimers;
use crate::pane::tracked_process::TrackedProcessChanges;
use crate::render_state::ClientRenderDmg;
use crate::render_state::ClientSessionFlow;
use crate::session::runtime::SessionPaneOutputMessage;

pub async fn handle_pane_output_message(
    event: Option<SessionPaneOutputMessage>,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    render_dmg: &mut ClientRenderDmg,
) -> rootcause::Result<ClientSessionFlow> {
    match event {
        Some(SessionPaneOutputMessage::PaneExited) => {
            match crate::client::session::handle_reaped_panes(state, event_writer, timers).await? {
                ReapedPanes::Unchanged => {}
                ReapedPanes::LayoutChanged => {
                    timers
                        .sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
                }
                ReapedPanes::Stop => return Ok(ClientSessionFlow::Disconnect),
            }
            Ok(ClientSessionFlow::Continue)
        }
        Some(SessionPaneOutputMessage::PaneOutputReady) => {
            let screen_dirty_panes = state.runtimes.take_screen_dirty_panes();
            let title_changes = state.runtimes.take_title_changes();
            let screen_dirty = !screen_dirty_panes.is_empty();
            let screen_dirty_visible = crate::screen_render::pane_ids_visible_render_dmg(
                state.layout,
                &state.pane_fullscreen,
                &state.terminal_size,
                &screen_dirty_panes,
            )?;
            // PTY output from hidden panes can still update titles/tracked-process state, but it must not make the
            // attached client rebuild the visible frame when the effective pane layout cannot show those cells.
            render_dmg.include_dmg(screen_dirty_visible);
            // Start the coalescing window before bounded writer sends below; otherwise slow sends add another frame.
            timers.sync_render_deadline(*render_dmg)?;
            let now = Instant::now();
            let tracked_process_changes = if screen_dirty {
                state.pane_tracked_processes.observe_runtime_visible_activity(
                    state.config.user_config.as_ref(),
                    state.runtimes,
                    &screen_dirty_panes,
                    now,
                )?
            } else {
                TrackedProcessChanges::default()
            };
            if !title_changes.is_empty()
                && crate::screen_render::flush_cmd_label_layout(event_writer, state, title_changes).await?
                    == ClientSessionFlow::Disconnect
            {
                return Ok(ClientSessionFlow::Disconnect);
            }
            if tracked_process_changes.state_change()
                == crate::pane::tracked_process::TrackedProcessStateChange::Changed
                && crate::screen_render::flush_tracked_process_runtime_layout(
                    timers,
                    event_writer,
                    state,
                    render_dmg,
                    screen_dirty_visible,
                )
                .await?
                    == ClientSessionFlow::Disconnect
            {
                return Ok(ClientSessionFlow::Disconnect);
            }
            // PTY exit status is sticky state. Detached exits wake the server loop through `pane_exit_notify`; while
            // attached, the bounded output channel is only a wakeup hint, so sweep the sticky state here.
            let reap = crate::client::session::handle_reaped_panes(state, event_writer, timers).await?;
            let layout_changed = match reap {
                ReapedPanes::Unchanged => false,
                ReapedPanes::LayoutChanged => true,
                ReapedPanes::Stop => return Ok(ClientSessionFlow::Disconnect),
            };
            if tracked_process_changes.deadline_change()
                == crate::pane::tracked_process::TrackedProcessDeadlineChange::Changed
                || layout_changed
            {
                // Visible tracked output can move only the quiet deadline while leaving the sidebar state unchanged.
                // Reap/restore can change the focused pane, so both cases need one sync after the output turn settles.
                timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
            }
            Ok(ClientSessionFlow::Continue)
        }
        None => Ok(ClientSessionFlow::Disconnect),
    }
}
