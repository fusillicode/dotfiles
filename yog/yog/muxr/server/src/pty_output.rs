use std::time::Instant;

use muxr_transport::ServerEventWriter;

use crate::client::session::ClientSessionState;
use crate::client::timers::ClientTimers;
use crate::session::runtime::SessionPaneOutputMessage;

pub async fn handle_pane_output_message(
    event: Option<SessionPaneOutputMessage>,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match event {
        Some(SessionPaneOutputMessage::PaneExited) => {
            Ok(!crate::client::session::handle_reaped_panes(state, event_writer).await?)
        }
        Some(SessionPaneOutputMessage::PaneOutputReady) => {
            let screen_dirty_panes = state.runtimes.take_screen_dirty_panes();
            let title_changes = state.runtimes.take_title_changes()?;
            let screen_dirty = !screen_dirty_panes.is_empty();
            let screen_dirty_visible = crate::screen_render::pane_ids_include_visible(
                state.layout,
                &state.pane_fullscreen,
                &state.terminal_size,
                &screen_dirty_panes,
            )?;
            // PTY output from hidden panes can still update titles/tracked-process state, but it must not make the
            // attached client rebuild the visible frame when the effective pane layout cannot show those cells.
            *render_dirty |= screen_dirty_visible;
            // Start the coalescing window before bounded writer sends below; otherwise slow sends add another frame.
            timers.sync_render_deadline(*render_dirty)?;
            let now = Instant::now();
            let tracked_process_changed = if screen_dirty {
                state.pane_tracked_processes.observe_runtime_visible_activity(
                    state.config.user_config.as_ref(),
                    state.runtimes,
                    &screen_dirty_panes,
                    now,
                )?
            } else {
                false
            };
            if !title_changes.is_empty()
                && !crate::screen_render::flush_cmd_label_layout(event_writer, state, title_changes).await?
            {
                return Ok(false);
            }
            if tracked_process_changed
                && !crate::screen_render::flush_tracked_process_runtime_layout(
                    timers,
                    event_writer,
                    state,
                    render_dirty,
                    screen_dirty_visible,
                )
                .await?
            {
                return Ok(false);
            }
            timers.sync_tracked_process_quiet_deadline(state.pane_tracked_processes.next_quiet_deadline()?)?;
            // PTY exit status is sticky state. Detached exits wake the server loop through `pane_exit_notify`; while
            // attached, the bounded output channel is only a wakeup hint, so sweep the sticky state here.
            if crate::client::session::handle_reaped_panes(state, event_writer).await? {
                return Ok(false);
            }
            Ok(true)
        }
        None => Ok(false),
    }
}
