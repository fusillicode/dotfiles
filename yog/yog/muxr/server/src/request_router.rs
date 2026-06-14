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
        ClientRequest::Detach => self::send_detached_event(event_writer, state).await,
        ClientRequest::DeleteSession => {
            crate::session::delete::handle_client_delete(
                event_writer,
                state.delete_sessions,
                state.config.client_write_timeout,
            )
            .await?;
            Ok(false)
        }
        ClientRequest::Input(bytes) => self::apply_pane_input_outcome(
            crate::pane::input::handle_client_input(&bytes, state)?,
            timers,
            render_dirty,
        ),
        ClientRequest::Paste(bytes) => self::apply_pane_input_outcome(
            crate::pane::input::handle_client_paste(&bytes, state)?,
            timers,
            render_dirty,
        ),
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
            )
            .await
        }
        ClientRequest::FocusTab(tab_id) => {
            self::apply_tab_focus_outcome(
                crate::tab::focus::handle_focus_tab_client_request(tab_id, state)?,
                event_writer,
                state,
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

fn apply_pane_input_outcome(
    outcome: PaneInputOutcome,
    timers: &mut ClientTimers,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let PaneInputOutcome {
        cmd_handoff_pane_id,
        render_dirty: input_render_dirty,
        sync_render_deadline,
    } = outcome;
    *render_dirty |= input_render_dirty;
    if let Some(pane_id) = cmd_handoff_pane_id {
        timers.schedule_cmd_handoff_sample(pane_id)?;
    }
    if sync_render_deadline {
        timers.sync_render_deadline(*render_dirty)?;
    }
    Ok(true)
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
    } = outcome;
    *render_dirty |= mouse_render_dirty;
    if sync_render_deadline {
        timers.sync_render_deadline(*render_dirty)?;
    }
    self::apply_pane_focus_outcome(focus, event_writer, state).await
}

async fn apply_pane_focus_outcome(
    outcome: PaneFocusClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<bool> {
    match outcome {
        PaneFocusClientOutcome::Focused {
            render: PaneFocusRender::ResizePanesAndRender,
        } => crate::screen_render::resize_panes_and_render(event_writer, state).await,
        PaneFocusClientOutcome::Focused {
            render: PaneFocusRender::SendLayoutAndBaseline,
        } => crate::screen_render::send_layout_and_baseline(event_writer, state).await,
        PaneFocusClientOutcome::Unchanged => Ok(true),
    }
}

async fn send_detached_event(
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<bool> {
    self::restore_scrollback_editor_without_render(state)?;
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
        KeyResolution::Cmd(cmd) => self::handle_cmd_request(cmd, event_writer, state).await,
        KeyResolution::Raw => self::apply_pane_input_outcome(
            crate::pane::input::handle_client_key(&key, state)?,
            timers,
            render_dirty,
        ),
    }
}

async fn handle_cmd_request(
    cmd: ClientCmd,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
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
            self::restore_scrollback_editor_without_render(state)?;
            crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
            return crate::screen_render::resize_panes_and_render(event_writer, state).await;
        }
        crate::scrollback_editor::ScrollbackEditorCmdAction::Run(cmd) => cmd,
    };
    match cmd {
        ClientCmd::Tab(cmd) => self::handle_tab_cmd_request(cmd, event_writer, state).await,
        ClientCmd::SplitPane(split_axis) => {
            self::apply_pane_split_outcome(
                crate::pane::split::handle_split_pane_cmd_client(split_axis, state)?,
                event_writer,
                state,
            )
            .await
        }
        ClientCmd::ClosePane => {
            self::apply_pane_close_outcome(
                crate::pane::close::handle_close_pane_cmd_client(state)?,
                event_writer,
                state,
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
            )
            .await
        }
        ClientCmd::FocusPane(direction) => {
            self::apply_pane_focus_outcome(
                crate::pane::focus::handle_focus_pane_cmd_client(direction, state)?,
                event_writer,
                state,
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

fn restore_scrollback_editor_without_render(state: &mut ClientSessionState<'_>) -> rootcause::Result<()> {
    if let Some(editor_pane_id) = crate::scrollback_editor::restore_without_render(state)?.editor_pane_id {
        crate::client::session::remove_pane_sink(state, editor_pane_id);
    }
    Ok(())
}

async fn apply_scrollback_editor_open_outcome(
    outcome: ScrollbackEditorOpenClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
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
            crate::screen_render::resize_panes_and_render(event_writer, state).await
        }
    }
}

async fn apply_pane_split_outcome(
    outcome: PaneSplitClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<bool> {
    let PaneSplitClientOutcome {
        new_pane_id,
        previous_pane,
    } = outcome;
    crate::client::session::attach_pane_sink_to_state(state, new_pane_id)?;
    crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
    crate::screen_render::resize_panes_and_render(event_writer, state).await
}

async fn apply_pane_close_outcome(
    outcome: ClosePaneClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<bool> {
    // Sink guards and detach events are client-shell resources; pane_close reports which pane disappeared.
    match outcome {
        ClosePaneClientOutcome::Final { pane_id } => {
            crate::client::session::remove_pane_sink(state, pane_id);
            self::send_detached_event(event_writer, state).await
        }
        ClosePaneClientOutcome::Removed { pane_id, previous_pane } => {
            crate::client::session::remove_pane_sink(state, pane_id);
            crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
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
) -> rootcause::Result<bool> {
    // Zero-field outcomes are intentionally avoided here; tab moves expose no shell data, unlike create/focus.
    match cmd {
        TabCmd::Create => {
            self::apply_tab_create_outcome(
                crate::tab::create::handle_create_tab_cmd_client(state)?,
                event_writer,
                state,
            )
            .await
        }
        TabCmd::FocusPrevious => {
            self::apply_tab_focus_outcome(
                crate::tab::focus::handle_focus_previous_tab_cmd_client(state)?,
                event_writer,
                state,
            )
            .await
        }
        TabCmd::FocusNext => {
            self::apply_tab_focus_outcome(
                crate::tab::focus::handle_focus_next_tab_cmd_client(state)?,
                event_writer,
                state,
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
) -> rootcause::Result<bool> {
    let TabCreateClientOutcome {
        new_pane_id,
        previous_pane,
    } = outcome;
    crate::client::session::attach_pane_sink_to_state(state, new_pane_id)?;
    crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
    crate::screen_render::resize_panes_and_render(event_writer, state).await
}

async fn apply_tab_focus_outcome(
    outcome: TabFocusClientOutcome,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<bool> {
    match outcome {
        TabFocusClientOutcome::Render { previous_pane } => {
            crate::pane::focus::write_active_pane_focus_events(previous_pane, state)?;
            crate::screen_render::resize_panes_and_render(event_writer, state).await
        }
        TabFocusClientOutcome::Unchanged => Ok(true),
    }
}
