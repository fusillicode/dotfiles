use muxr_core::ClientKey;
use muxr_core::ClientRequest;
use muxr_core::ServerError;
use muxr_core::ServerEvent;
use muxr_transport::ServerEventWriter;

use crate::client::session::ClientSessionState;
use crate::client::timers::ClientTimers;
use crate::keyboard_input::ClientCmd;
use crate::keyboard_input::KeyResolution;
use crate::keyboard_input::TabCmd;
use crate::pane::close::ClosePaneClientOutcome;
use crate::pane::focus::PaneFocusClientOutcome;
use crate::pane::focus::PaneFocusRender;
use crate::pane::input::PaneInputOutcome;
use crate::pane::mouse::PaneMouseClientOutcome;
use crate::pane::resize::PaneResizeClientOutcome;
use crate::pane::resize::PaneResizeRender;
use crate::pane::scroll::PaneScrollLineRequestOutcome;
use crate::pane::split::PaneSplitClientOutcome;
use crate::pane::tracked_process::TrackedProcessChanges;
use crate::pane::tracked_process::TrackedProcessClientChange;
use crate::scrollback_editor::ScrollbackEditorOpenClientOutcome;
use crate::session::runtime::SessionClientMessage;
use crate::tab::create::TabCreateClientOutcome;
use crate::tab::focus::TabFocusClientOutcome;

pub async fn handle_client_message(
    message: SessionClientMessage,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    heartbeat_started_at: &mut Option<tokio::time::Instant>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match message {
        SessionClientMessage::ClientDisconnected => Ok(false),
        SessionClientMessage::Request(request) => {
            self::handle_client_request(request, event_writer, state, timers, heartbeat_started_at, render_dirty).await
        }
    }
}

async fn handle_client_request(
    request: ClientRequest,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    heartbeat_started_at: &mut Option<tokio::time::Instant>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match request {
        ClientRequest::Detach => self::send_detached_event(event_writer, state, timers).await,
        ClientRequest::DeleteSession => {
            crate::session::delete::handle_client_delete(
                event_writer,
                state.delete_sessions,
                state.config.client_write_timeout,
            )
            .await?;
            Ok(false)
        }
        ClientRequest::Input(bytes) => {
            self::handle_input_request(&bytes, event_writer, state, timers, render_dirty).await
        }
        ClientRequest::Paste(bytes) => {
            self::handle_paste_request(&bytes, event_writer, state, timers, render_dirty).await
        }
        ClientRequest::Key(key) => self::handle_key_request(key, event_writer, state, timers, render_dirty).await,
        ClientRequest::Mouse(event) => {
            self::apply_pane_mouse_outcome(
                crate::pane::mouse::handle_mouse_event_client_request(event, state)?,
                event_writer,
                state,
                timers,
                render_dirty,
            )
            .await
        }
        ClientRequest::ScrollPaneLineAt { position, direction } => {
            self::apply_pane_scroll_line_outcome(
                crate::pane::scroll::handle_scroll_pane_line_client_request(position, direction, state)?,
                event_writer,
                state,
                timers,
                render_dirty,
            )
            .await
        }
        ClientRequest::FocusPaneAt(position) => {
            self::apply_pane_focus_outcome(
                crate::pane::focus::handle_focus_pane_at_client_request(position, state)?,
                event_writer,
                state,
                timers,
            )
            .await
        }
        ClientRequest::FocusTab(tab_id) => {
            self::apply_tab_focus_outcome(
                crate::tab::focus::handle_focus_tab_client_request(tab_id, state)?,
                event_writer,
                state,
                timers,
            )
            .await
        }
        ClientRequest::Resize(size) => {
            state.terminal_size = size;
            if !crate::screen_render::resize_panes_and_render(event_writer, state).await? {
                return Ok(false);
            }
            Ok(true)
        }
        ClientRequest::RenderResync => {
            if !crate::screen_render::send_layout_and_baseline(event_writer, state).await? {
                return Ok(false);
            }
            Ok(true)
        }
        ClientRequest::Ping => {
            crate::event_writer::send_event_with_timeout(
                event_writer,
                &ServerEvent::Pong,
                state.config.client_write_timeout,
            )
            .await
        }
        ClientRequest::Pong => {
            *heartbeat_started_at = None;
            Ok(true)
        }
        request @ ClientRequest::Attach(_) => {
            let _sent = crate::event_writer::send_event_with_timeout(
                event_writer,
                &ServerEvent::Error(ServerError::unexpected_request(request)),
                state.config.client_write_timeout,
            )
            .await?;
            Ok(false)
        }
    }
}

async fn handle_input_request(
    bytes: &[u8],
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    self::apply_pane_input_outcome(
        crate::pane::input::handle_client_input(bytes, state)?,
        event_writer,
        state,
        timers,
        render_dirty,
    )
    .await
}

async fn handle_paste_request(
    bytes: &[u8],
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    self::apply_pane_input_outcome(
        crate::pane::input::handle_client_paste(bytes, state)?,
        event_writer,
        state,
        timers,
        render_dirty,
    )
    .await
}

async fn apply_pane_input_outcome(
    outcome: PaneInputOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    self::apply_pane_input_timers(&outcome, timers, render_dirty)?;
    if let Some(tracked_process_change) = outcome.tracked_process_change
        && !self::apply_tracked_process_client_change(tracked_process_change, event_writer, state, timers, render_dirty)
            .await?
    {
        return Ok(false);
    }
    Ok(true)
}

fn apply_pane_input_timers(
    outcome: &PaneInputOutcome,
    timers: &mut ClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<()> {
    *render_dirty |= outcome.render_dirty;
    if outcome.interactive_render {
        timers.record_interactive_input()?;
    }
    if let Some(pane_id) = outcome.cmd_handoff_pane_id {
        timers.schedule_cmd_handoff_sample(pane_id)?;
    }
    if outcome.sync_render_deadline {
        timers.sync_render_deadline(*render_dirty)?;
    }
    Ok(())
}

async fn apply_pane_scroll_line_outcome(
    outcome: PaneScrollLineRequestOutcome,
    event_writer: &mut ServerEventWriter,
    state: &ClientSessionState<'_>,
    timers: &mut ClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let PaneScrollLineRequestOutcome {
        event,
        render_dirty: scroll_render_dirty,
    } = outcome;
    *render_dirty |= scroll_render_dirty;
    timers.sync_render_deadline(*render_dirty)?;
    crate::event_writer::send_event_with_timeout(event_writer, &event, state.config.client_write_timeout).await
}

async fn apply_pane_mouse_outcome(
    outcome: PaneMouseClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let PaneMouseClientOutcome {
        focus,
        render_dirty: mouse_render_dirty,
        sync_render_deadline,
        tracked_process_change,
    } = outcome;
    *render_dirty |= mouse_render_dirty;
    if sync_render_deadline {
        timers.sync_render_deadline(*render_dirty)?;
    }
    if let Some(tracked_process_change) = tracked_process_change
        && !self::apply_tracked_process_client_change(tracked_process_change, event_writer, state, timers, render_dirty)
            .await?
    {
        return Ok(false);
    }
    self::apply_pane_focus_outcome(focus, event_writer, state, timers).await
}

async fn apply_tracked_process_client_change(
    tracked_process_change: TrackedProcessClientChange,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let changes = tracked_process_change.changes();
    if self::tracked_process_change_needs_deadline_sync(changes, timers) {
        timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
    }
    if !changes.state_changed() {
        return Ok(true);
    }

    let pane_id = tracked_process_change.pane_id();
    let pane_surface_dirty = crate::screen_render::pane_ids_include_visible(
        state.layout,
        &state.pane_fullscreen,
        &state.terminal_size,
        &[pane_id],
    )?;
    crate::screen_render::flush_tracked_process_runtime_layout(
        timers,
        event_writer,
        state,
        render_dirty,
        pane_surface_dirty,
    )
    .await
}

fn tracked_process_change_needs_deadline_sync(changes: TrackedProcessChanges, timers: &ClientTimers) -> bool {
    // Deadline-only local echo can wait for the existing sleep to re-check; sync immediately only when that sleep is
    // already ready, or when a sidebar state change must publish a new Busy/Seen/Unseen snapshot.
    changes.state_changed() || (changes.deadline_changed() && timers.tracked_process_quiet_sleep_deadline_has_passed())
}

async fn apply_pane_focus_outcome(
    outcome: PaneFocusClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
) -> rootcause::Result<bool> {
    let PaneFocusClientOutcome::Focused { render } = outcome else {
        return Ok(true);
    };
    timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
    match render {
        PaneFocusRender::ResizePanesAndRender => {
            crate::screen_render::resize_panes_and_render(event_writer, state).await
        }
        PaneFocusRender::SendLayoutAndBaseline => {
            crate::screen_render::send_layout_and_baseline(event_writer, state).await
        }
    }
}

async fn send_detached_event(
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
) -> rootcause::Result<bool> {
    self::restore_scrollback_editor_without_render(state, timers)?;
    crate::client::lifecycle::record_detach_ack_send_failure(
        crate::event_writer::send_event_failure(
            event_writer,
            &ServerEvent::Detached,
            state.config.client_write_timeout,
        )
        .await,
    );
    Ok(false)
}

async fn handle_key_request(
    key: ClientKey,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match crate::keyboard_input::resolve_key(&mut state.input_mode, &key) {
        KeyResolution::Cmd(cmd) => self::handle_cmd_request(cmd, event_writer, state, timers).await,
        KeyResolution::Raw => {
            self::apply_pane_input_outcome(
                crate::pane::input::handle_client_key(&key, state)?,
                event_writer,
                state,
                timers,
                render_dirty,
            )
            .await
        }
    }
}

async fn handle_cmd_request(
    cmd: ClientCmd,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
) -> rootcause::Result<bool> {
    let cmd = match crate::scrollback_editor::cmd_action(cmd, state.scrollback_editor.is_some()) {
        crate::scrollback_editor::ScrollbackEditorCmdAction::Ignore => {
            // The editor pane is attached-client-local. Muxr layout shortcuts are blocked while it is active so they
            // cannot create temporary panes/runtimes that disappear from the restored real layout.
            return Ok(true);
        }
        crate::scrollback_editor::ScrollbackEditorCmdAction::Restore => {
            let previous_pane = state.layout.active_pane_id()?;
            crate::scrollback_editor::write_focus_lost_if_live(state.scrollback_editor.as_ref(), state.runtimes)?;
            self::restore_scrollback_editor_without_render(state, timers)?;
            crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
            timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
            return crate::screen_render::resize_panes_and_render(event_writer, state).await;
        }
        crate::scrollback_editor::ScrollbackEditorCmdAction::Run(cmd) => cmd,
    };
    match cmd {
        ClientCmd::Tab(cmd) => self::handle_tab_cmd_request(cmd, event_writer, state, timers).await,
        ClientCmd::SplitPane(split_axis) => {
            self::apply_pane_split_outcome(
                crate::pane::split::handle_split_pane_cmd_client(split_axis, state)?,
                event_writer,
                state,
                timers,
            )
            .await
        }
        ClientCmd::ClosePane => {
            self::apply_pane_close_outcome(
                crate::pane::close::handle_close_pane_cmd_client(state)?,
                event_writer,
                state,
                timers,
            )
            .await
        }
        ClientCmd::ResizePane(direction) => {
            self::apply_pane_resize_outcome(
                crate::pane::resize::handle_resize_pane_cmd_client(direction, state)?,
                event_writer,
                state,
            )
            .await
        }
        ClientCmd::OpenScrollbackEditor => {
            self::apply_scrollback_editor_open_outcome(
                crate::scrollback_editor::handle_open_client_request(
                    state.config.user_config.scrollback.dump_style,
                    state,
                )?,
                event_writer,
                state,
                timers,
            )
            .await
        }
        ClientCmd::FocusPane(direction) => {
            self::apply_pane_focus_outcome(
                crate::pane::focus::handle_focus_pane_cmd_client(direction, state)?,
                event_writer,
                state,
                timers,
            )
            .await
        }
        ClientCmd::EnterResizeMode => {
            self::apply_pane_resize_outcome(
                crate::pane::resize::handle_enter_resize_mode_cmd_client(state),
                event_writer,
                state,
            )
            .await
        }
        ClientCmd::ExitMode => {
            self::apply_pane_resize_outcome(
                crate::pane::resize::handle_exit_resize_mode_cmd_client(),
                event_writer,
                state,
            )
            .await
        }
        ClientCmd::TogglePaneFullscreen => {
            crate::pane::fullscreen::handle_toggle_active_pane_cmd_client(state)?;
            // Zero-field outcomes are intentionally avoided: fullscreen exposes no shell data, and always redraws.
            crate::screen_render::resize_panes_and_render(event_writer, state).await
        }
    }
}

fn restore_scrollback_editor_without_render(
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
) -> rootcause::Result<()> {
    if let Some(editor_pane_id) = crate::scrollback_editor::restore_without_render(state)?.editor_pane_id {
        crate::client::session::remove_pane_from_client_state(state, timers, editor_pane_id)?;
    }
    Ok(())
}

async fn apply_scrollback_editor_open_outcome(
    outcome: ScrollbackEditorOpenClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
) -> rootcause::Result<bool> {
    match outcome {
        ScrollbackEditorOpenClientOutcome::AlreadyOpen => Ok(true),
        ScrollbackEditorOpenClientOutcome::Opened {
            editor,
            editor_pane_id,
            previous_pane,
        } => {
            if let Err(error) = crate::client::session::attach_pane_sink_to_state(state, editor_pane_id) {
                crate::scrollback_editor::rollback_open_client_request(state, editor)?;
                return Err(error);
            }
            state.scrollback_editor = Some(editor);
            crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
            timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
            crate::screen_render::resize_panes_and_render(event_writer, state).await
        }
    }
}

async fn apply_pane_split_outcome(
    outcome: PaneSplitClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
) -> rootcause::Result<bool> {
    let PaneSplitClientOutcome {
        new_pane_id,
        previous_pane,
    } = outcome;
    crate::client::session::attach_pane_sink_to_state(state, new_pane_id)?;
    crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
    timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
    crate::screen_render::resize_panes_and_render(event_writer, state).await
}

async fn apply_pane_close_outcome(
    outcome: ClosePaneClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
) -> rootcause::Result<bool> {
    // `remove_pane_from_client_state` is the client cleanup boundary for sink guards, tracked state, and handoff
    // timers. pane_close reports which pane disappeared; this layer owns the attached-client resources.
    match outcome {
        ClosePaneClientOutcome::Final { pane_id } => {
            crate::client::session::remove_pane_from_client_state(state, timers, pane_id)?;
            self::send_detached_event(event_writer, state, timers).await
        }
        ClosePaneClientOutcome::Removed { pane_id, previous_pane } => {
            crate::client::session::remove_pane_from_client_state(state, timers, pane_id)?;
            crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
            crate::client::session::acknowledge_active_tracked_process(state)?;
            timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
            crate::screen_render::resize_panes_and_render(event_writer, state).await
        }
    }
}

async fn apply_pane_resize_outcome(
    outcome: PaneResizeClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<bool> {
    match outcome {
        PaneResizeClientOutcome::Render {
            render: PaneResizeRender::ResizePanesAndRender,
        } => crate::screen_render::resize_panes_and_render(event_writer, state).await,
        PaneResizeClientOutcome::Render {
            render: PaneResizeRender::SendLayoutAndBaseline,
        } => crate::screen_render::send_layout_and_baseline(event_writer, state).await,
        PaneResizeClientOutcome::Unchanged => Ok(true),
    }
}

async fn handle_tab_cmd_request(
    cmd: TabCmd,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
) -> rootcause::Result<bool> {
    // Tab moves only reorder and redraw. Create/focus carry pane ids for sinks, focus events, and timer sync.
    match cmd {
        TabCmd::Create => {
            self::apply_tab_create_outcome(
                crate::tab::create::handle_create_tab_cmd_client(state)?,
                event_writer,
                state,
                timers,
            )
            .await
        }
        TabCmd::FocusPrevious => {
            self::apply_tab_focus_outcome(
                crate::tab::focus::handle_focus_previous_tab_cmd_client(state)?,
                event_writer,
                state,
                timers,
            )
            .await
        }
        TabCmd::FocusNext => {
            self::apply_tab_focus_outcome(
                crate::tab::focus::handle_focus_next_tab_cmd_client(state)?,
                event_writer,
                state,
                timers,
            )
            .await
        }
        TabCmd::MovePrevious => {
            crate::tab::r#move::handle_move_active_tab_previous_cmd_client(state)?;
            crate::screen_render::resize_panes_and_render(event_writer, state).await
        }
        TabCmd::MoveNext => {
            crate::tab::r#move::handle_move_active_tab_next_cmd_client(state)?;
            crate::screen_render::resize_panes_and_render(event_writer, state).await
        }
    }
}

async fn apply_tab_create_outcome(
    outcome: TabCreateClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
) -> rootcause::Result<bool> {
    let TabCreateClientOutcome {
        new_pane_id,
        previous_pane,
    } = outcome;
    crate::client::session::attach_pane_sink_to_state(state, new_pane_id)?;
    crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
    timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
    crate::screen_render::resize_panes_and_render(event_writer, state).await
}

async fn apply_tab_focus_outcome(
    outcome: TabFocusClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    timers: &mut ClientTimers,
) -> rootcause::Result<bool> {
    match outcome {
        TabFocusClientOutcome::Render { previous_pane } => {
            crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
            timers.sync_tracked_process_quiet_deadline_for_layout(&state.pane_tracked_processes, state.layout)?;
            crate::screen_render::resize_panes_and_render(event_writer, state).await
        }
        TabFocusClientOutcome::Unchanged => Ok(true),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use muxr_core::PaneId;

    use super::*;
    use crate::pane::cmd::PaneCmd;
    use crate::pane::cmd::PaneCmdObservation;
    use crate::pane::split::PaneSplitAxis;
    use crate::pane::tracked_process::PaneTrackedProcesses;
    use crate::pane::tracked_process::TrackedProcessUserInteraction;
    use crate::state::SessionLayout;
    use crate::state::SessionMetadata;

    #[tokio::test(start_paused = true)]
    async fn test_sync_tracked_process_quiet_deadline_when_focus_changes_shortens_focused_input_deadline()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        let other_pane_id = PaneId::new(2)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        pane_tracked_processes.record_user_interaction(
            &layout,
            pane_id,
            TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_secs(2))?,
        )?;

        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
        let focused_deadline = timers.tracked_process_quiet_sleep.deadline();

        layout.active_tab_mut()?.focus_pane(other_pane_id)?;
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;

        assert2::assert!(timers.tracked_process_quiet_sleep.deadline() < focused_deadline);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn test_apply_pane_input_timers_when_input_is_interactive_shortens_pending_bulk_render_deadline()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;
        timers.sync_render_deadline(true)?;
        timers.complete_render_frame()?;
        timers.sync_render_deadline(true)?;
        let bulk_deadline = timers.render_sleep.deadline();
        let mut render_dirty = true;

        apply_pane_input_timers(
            &PaneInputOutcome {
                cmd_handoff_pane_id: None,
                interactive_render: true,
                render_dirty: false,
                sync_render_deadline: true,
                tracked_process_change: None,
            },
            &mut timers,
            &mut render_dirty,
        )?;

        assert2::assert!(timers.render_sleep.deadline() < bulk_deadline);
        assert2::assert!(render_dirty);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn test_tracked_process_change_needs_deadline_sync_when_deadline_only_waits_for_ready_sleep()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        let changes = pane_tracked_processes.record_user_interaction(
            &layout,
            pane_id,
            TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_secs(2))?,
        )?;
        let mut timers = ClientTimers::new(&config)?;

        assert2::assert!(!self::tracked_process_change_needs_deadline_sync(changes, &timers));

        timers
            .tracked_process_quiet_sleep
            .as_mut()
            .reset(tokio::time::Instant::now());
        assert2::assert!(self::tracked_process_change_needs_deadline_sync(changes, &timers));
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn test_apply_pane_input_timers_when_input_is_not_interactive_keeps_pending_bulk_render_deadline()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;
        timers.sync_render_deadline(true)?;
        timers.complete_render_frame()?;
        timers.sync_render_deadline(true)?;
        let bulk_deadline = timers.render_sleep.deadline();
        let mut render_dirty = true;

        apply_pane_input_timers(
            &PaneInputOutcome {
                cmd_handoff_pane_id: None,
                interactive_render: false,
                render_dirty: false,
                sync_render_deadline: true,
                tracked_process_change: None,
            },
            &mut timers,
            &mut render_dirty,
        )?;

        pretty_assertions::assert_eq!(timers.render_sleep.deadline(), bulk_deadline);
        assert2::assert!(render_dirty);
        Ok(())
    }

    fn layout(config: &crate::server::ServerConfig) -> rootcause::Result<SessionLayout> {
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.split_active_pane(
            config.user_config.layout,
            self::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        Ok(layout)
    }

    fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }

    fn fg_tracked_process(executable: &str) -> PaneCmdObservation {
        PaneCmdObservation::FgCmd(crate::pane::cmd::FgCmd::from_test_cmd(PaneCmd {
            executable: executable.to_owned(),
            path: None,
            pid: 42,
        }))
    }

    fn instant_after(instant: Instant, duration: Duration) -> rootcause::Result<Instant> {
        instant
            .checked_add(duration)
            .ok_or_else(|| rootcause::report!("test instant overflowed"))
    }
}
