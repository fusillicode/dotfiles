use std::io::Write;

use muxr_config::MuxrConfig;
use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::ClientMousePosition;
use muxr_core::ClientRequest;

use super::DroppableSendOutcome;
use super::copy_selection::SelectionInput;
use super::pane_focus::LocalMouseAction;
use super::renderer::ClientRenderer;

pub async fn handle_mouse_input_action(
    muxr_config: &MuxrConfig,
    event: ClientMouseEvent,
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    renderer: &mut ClientRenderer,
    stdout: &mut impl Write,
) -> rootcause::Result<bool> {
    let Some(position) = pane_position(muxr_config, event.position) else {
        if let Some(request) = tab_focus_request_for_sidebar_click(event, renderer) {
            if input_sender.send(request).await.is_err() {
                return Ok(false);
            }
            return Ok(true);
        }
        // Captured app drags can finish over the tab bar; forward them clamped to the captured pane before dropping
        // ordinary tab bar mouse packets.
        if renderer.has_mouse_capture()
            && let Some(position) = pane_position_for_sidebar_drag(muxr_config, event.position)
            && let Some(event) = renderer.mouse_request_for_event(ClientMouseEvent { position, ..event })
        {
            return send_mouse_request(input_sender, event).await;
        }
        // Local selections can also finish over the tab bar; keep update/end routed so the retained pane drag is
        // clamped and finalized instead of leaving stale drag state behind.
        let tab_bar_position = event.position;
        match super::pane_focus::local_mouse_action(event) {
            Some(LocalMouseAction::SelectionUpdate(_)) => {
                if let Some(position) = pane_position_for_sidebar_drag(muxr_config, tab_bar_position) {
                    let scroll_request = renderer.set_selection_outside_edge_drag(position);
                    renderer.apply_selection_input(stdout, SelectionInput::Update(position))?;
                    if let Some(request) = scroll_request {
                        return Ok(super::send_edge_scroll_request(input_sender, renderer, request));
                    }
                }
            }
            Some(LocalMouseAction::SelectionEnd(_)) => {
                if let Some(position) = pane_position_for_sidebar_drag(muxr_config, tab_bar_position) {
                    renderer.apply_selection_input(stdout, SelectionInput::End(position))?;
                }
            }
            Some(LocalMouseAction::FocusAndSelectionStart(_)) | None => {}
        }
        return Ok(true);
    };
    let event = ClientMouseEvent { position, ..event };
    if super::pane_scroll::is_wheel_event(event) {
        return send_mouse_request(input_sender, event).await;
    }
    if let Some(event) = renderer.mouse_request_for_event(event) {
        return send_mouse_request(input_sender, event).await;
    }

    match super::pane_focus::local_mouse_action(event) {
        Some(LocalMouseAction::FocusAndSelectionStart(position)) => {
            if input_sender.send(ClientRequest::FocusPaneAt(position)).await.is_err() {
                return Ok(false);
            }
            renderer.apply_selection_input(stdout, SelectionInput::Start(position))?;
            Ok(true)
        }
        Some(LocalMouseAction::SelectionUpdate(position)) => {
            let scroll_request = renderer.set_selection_edge_drag(position, None);
            renderer.apply_selection_input(stdout, SelectionInput::Update(position))?;
            if let Some(request) = scroll_request {
                return Ok(super::send_edge_scroll_request(input_sender, renderer, request));
            }
            Ok(true)
        }
        Some(LocalMouseAction::SelectionEnd(position)) => {
            renderer.apply_selection_input(stdout, SelectionInput::End(position))?;
            Ok(true)
        }
        None => Ok(true),
    }
}

pub const fn mouse_event_can_be_dropped(event: ClientMouseEvent) -> bool {
    event.button & 32 != 0 && !super::pane_scroll::is_wheel_event(event)
}

async fn send_mouse_request(
    input_sender: &tokio::sync::mpsc::Sender<ClientRequest>,
    event: ClientMouseEvent,
) -> rootcause::Result<bool> {
    if mouse_event_can_be_dropped(event) {
        return Ok(!matches!(
            super::send_droppable_request(input_sender, ClientRequest::Mouse(event)),
            DroppableSendOutcome::Closed
        ));
    }
    if input_sender.send(ClientRequest::Mouse(event)).await.is_err() {
        return Ok(false);
    }
    Ok(true)
}

fn tab_focus_request_for_sidebar_click(event: ClientMouseEvent, renderer: &ClientRenderer) -> Option<ClientRequest> {
    if event.phase != ClientMouseEventPhase::Press || event.button != 0 {
        return None;
    }
    renderer
        .tab_id_at_sidebar_row(event.position.row)
        .map(ClientRequest::FocusTab)
}

fn pane_position(config: &MuxrConfig, position: ClientMousePosition) -> Option<ClientMousePosition> {
    Some(ClientMousePosition {
        row: position.row,
        col: position.col.checked_sub(config.tab_bar.width)?,
    })
}

const fn pane_position_for_sidebar_drag(
    config: &MuxrConfig,
    position: ClientMousePosition,
) -> Option<ClientMousePosition> {
    if position.col >= config.tab_bar.width {
        return None;
    }
    Some(ClientMousePosition {
        row: position.row,
        col: 0,
    })
}

#[cfg(test)]
mod tests {
    use muxr_core::LayoutSnapshot;
    use muxr_core::PaneId;
    use muxr_core::PaneRegionsSnapshot;
    use muxr_core::PaneSnapshot;
    use muxr_core::TabId;
    use muxr_core::TabSnapshot;
    use muxr_core::TerminalSize;
    use rootcause::prelude::ResultExt;
    use rootcause::report;

    use super::super::renderer::ClientRenderer;
    use super::super::renderer::test_helpers as renderer_test_helpers;
    use super::super::terminal::SynchronizedOutput;
    use super::*;

    #[test]
    fn test_handle_mouse_input_action_when_plain_mouse_click_arrives_focuses_pane() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let config = MuxrConfig::default();
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
            let mut renderer = ClientRenderer::with_synchronized_output(
                self::layout_snapshot()?,
                self::pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();

            assert2::assert!(
                handle_mouse_input_action(
                    &config,
                    ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Press,
                        position: ClientMousePosition {
                            row: 0,
                            col: config.tab_bar.width.saturating_add(1)
                        }
                    },
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::FocusPaneAt(ClientMousePosition { row: 0, col: 1 })),
            );
            Ok(())
        })
    }

    #[test]
    fn test_handle_mouse_input_action_when_tab_sidebar_is_clicked_focuses_tab() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let config = MuxrConfig::default();
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
            let mut renderer = ClientRenderer::with_synchronized_output(
                self::two_tab_layout()?,
                self::pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();

            assert2::assert!(
                handle_mouse_input_action(
                    &config,
                    ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Press,
                        position: ClientMousePosition { row: 3, col: 1 }
                    },
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::FocusTab(TabId::new(2)?)),
            );
            pretty_assertions::assert_eq!(output.flushes, 0);
            Ok(())
        })
    }

    #[test]
    fn test_handle_mouse_input_action_when_selection_release_is_on_tab_sidebar_finalizes_selection()
    -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let config = MuxrConfig::default();
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
            let mut renderer = ClientRenderer::with_synchronized_output(
                self::layout_snapshot()?,
                self::pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut initial_output = CountingWriter::default();
            renderer.apply_render(
                &mut initial_output,
                muxr_core::RenderUpdate::Baseline(self::render_baseline()?),
            )?;
            let mut output = CountingWriter::default();

            assert2::assert!(
                handle_mouse_input_action(
                    &config,
                    ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Press,
                        position: ClientMousePosition {
                            row: 0,
                            col: config.tab_bar.width.saturating_add(1)
                        }
                    },
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_mouse_input_action(
                    &config,
                    ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Release,
                        position: ClientMousePosition { row: 0, col: 1 }
                    },
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::FocusPaneAt(ClientMousePosition { row: 0, col: 1 })),
            );
            pretty_assertions::assert_eq!(renderer_test_helpers::selected_text(&renderer), Some("ab".to_owned()),);
            Ok(())
        })
    }

    #[test]
    fn test_handle_mouse_input_action_when_selection_drag_moves_into_tab_sidebar_clamps_to_left_edge()
    -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let config = MuxrConfig::default();
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(2);
            let mut renderer = ClientRenderer::with_synchronized_output(
                self::layout_snapshot()?,
                self::pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut initial_output = CountingWriter::default();
            renderer.apply_render(
                &mut initial_output,
                muxr_core::RenderUpdate::Baseline(self::render_baseline()?),
            )?;
            let mut output = CountingWriter::default();

            assert2::assert!(
                handle_mouse_input_action(
                    &config,
                    ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Press,
                        position: ClientMousePosition {
                            row: 0,
                            col: config.tab_bar.width.saturating_add(1)
                        }
                    },
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );
            assert2::assert!(
                handle_mouse_input_action(
                    &config,
                    ClientMouseEvent {
                        button: 32,
                        phase: ClientMouseEventPhase::Press,
                        position: ClientMousePosition { row: 0, col: 0 }
                    },
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::FocusPaneAt(ClientMousePosition { row: 0, col: 1 })),
            );
            assert2::assert!(matches!(
                input_receiver.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ));
            pretty_assertions::assert_eq!(renderer_test_helpers::selected_text(&renderer), Some("ab".to_owned()),);
            Ok(())
        })
    }

    #[test]
    fn test_handle_mouse_input_action_when_pane_tracks_mouse_forwards_mouse_to_server() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let config = MuxrConfig::default();
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
            let mut renderer = ClientRenderer::with_synchronized_output(
                self::layout_snapshot()?,
                self::mouse_tracking_pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();

            assert2::assert!(
                handle_mouse_input_action(
                    &config,
                    ClientMouseEvent {
                        button: 0,
                        phase: ClientMouseEventPhase::Press,
                        position: ClientMousePosition {
                            row: 0,
                            col: config.tab_bar.width.saturating_add(1)
                        }
                    },
                    &input_sender,
                    &mut renderer,
                    &mut output,
                )
                .await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    button: 0,
                    phase: ClientMouseEventPhase::Press,
                    position: ClientMousePosition { row: 0, col: 1 }
                })),
            );
            pretty_assertions::assert_eq!(output.flushes, 0);
            Ok(())
        })
    }

    #[test]
    fn test_handle_mouse_input_action_when_pane_receives_wheel_forwards_mouse_to_server() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let config = MuxrConfig::default();
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
            let mut renderer = ClientRenderer::with_synchronized_output(
                self::layout_snapshot()?,
                self::pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();
            let event = ClientMouseEvent {
                button: 64,
                phase: ClientMouseEventPhase::Press,
                position: ClientMousePosition {
                    row: 0,
                    col: config.tab_bar.width.saturating_add(1),
                },
            };

            assert2::assert!(
                handle_mouse_input_action(&config, event, &input_sender, &mut renderer, &mut output).await?
            );

            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    position: ClientMousePosition { row: 0, col: 1 },
                    ..event
                })),
            );
            pretty_assertions::assert_eq!(output.flushes, 0);
            Ok(())
        })
    }

    #[test]
    fn test_handle_mouse_input_action_when_pane_wheel_request_queue_is_full_waits_for_queue_space()
    -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let config = MuxrConfig::default();
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(1);
            assert2::assert!(input_sender.try_send(ClientRequest::Pong).is_ok());
            let mut renderer = ClientRenderer::with_synchronized_output(
                self::layout_snapshot()?,
                self::pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();
            let event = ClientMouseEvent {
                button: 64,
                phase: ClientMouseEventPhase::Press,
                position: ClientMousePosition {
                    row: 0,
                    col: config.tab_bar.width.saturating_add(1),
                },
            };
            let handle = handle_mouse_input_action(&config, event, &input_sender, &mut renderer, &mut output);
            tokio::pin!(handle);

            tokio::select! {
                result = &mut handle => {
                    return Err(report!("muxr wheel request did not wait for queue space").attach(format!("{result:?}")));
                }
                () = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
            }

            pretty_assertions::assert_eq!(input_receiver.recv().await, Some(ClientRequest::Pong));
            assert2::assert!(handle.await?);
            pretty_assertions::assert_eq!(
                input_receiver.recv().await,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    position: ClientMousePosition { row: 0, col: 1 },
                    ..event
                })),
            );
            Ok(())
        })
    }

    #[test]
    fn test_handle_mouse_input_action_when_tracking_drag_crosses_pane_routes_to_pressed_pane() -> rootcause::Result<()>
    {
        self::runtime()?.block_on(async {
            let config = MuxrConfig::default();
            let (input_sender, mut input_receiver) = tokio::sync::mpsc::channel(4);
            let mut renderer = ClientRenderer::with_synchronized_output(
                self::layout_snapshot()?,
                self::split_mouse_tracking_pane_regions_snapshot()?,
                SynchronizedOutput::Csi,
            );
            let mut output = CountingWriter::default();

            let events = [
                ClientMouseEvent {
                    button: 0,
                    phase: ClientMouseEventPhase::Press,
                    position: ClientMousePosition {
                        row: 0,
                        col: config.tab_bar.width.saturating_add(1),
                    },
                },
                ClientMouseEvent {
                    button: 32,
                    phase: ClientMouseEventPhase::Press,
                    position: ClientMousePosition {
                        row: 0,
                        col: config.tab_bar.width.saturating_add(3),
                    },
                },
                ClientMouseEvent {
                    button: 0,
                    phase: ClientMouseEventPhase::Release,
                    position: ClientMousePosition { row: 0, col: 1 },
                },
                ClientMouseEvent {
                    button: 32,
                    phase: ClientMouseEventPhase::Press,
                    position: ClientMousePosition {
                        row: 0,
                        col: config.tab_bar.width.saturating_add(3),
                    },
                },
            ];
            for event in events {
                assert2::assert!(
                    handle_mouse_input_action(&config, event, &input_sender, &mut renderer, &mut output).await?
                );
            }

            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    button: 0,
                    phase: ClientMouseEventPhase::Press,
                    position: ClientMousePosition { row: 0, col: 1 }
                })),
            );
            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    button: 32,
                    phase: ClientMouseEventPhase::Press,
                    position: ClientMousePosition { row: 0, col: 1 }
                })),
            );
            pretty_assertions::assert_eq!(
                self::recv_client_request(&mut input_receiver).await?,
                Some(ClientRequest::Mouse(ClientMouseEvent {
                    button: 0,
                    phase: ClientMouseEventPhase::Release,
                    position: ClientMousePosition { row: 0, col: 0 }
                })),
            );
            assert2::assert!(matches!(
                input_receiver.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ));
            Ok(())
        })
    }

    async fn recv_client_request(
        input_receiver: &mut tokio::sync::mpsc::Receiver<ClientRequest>,
    ) -> rootcause::Result<Option<ClientRequest>> {
        Ok(
            tokio::time::timeout(std::time::Duration::from_secs(1), input_receiver.recv())
                .await
                .context("timed out waiting for muxr client request")?,
        )
    }

    fn layout_snapshot() -> rootcause::Result<LayoutSnapshot> {
        let active_tab = TabId::new(1)?;
        let active_pane = PaneId::new(1)?;
        let pane = PaneSnapshot {
            tracked_process_state: muxr_core::TrackedProcessState::None,
            cwd: "/tmp".to_owned(),
            cmd_label: None,
            focus_seq: 1,
            git_stats: None,
            id: active_pane,
            title: "shell".to_owned(),
        };
        let tab = TabSnapshot::new(active_tab, "default", active_pane, vec![pane])?;
        LayoutSnapshot::new(active_tab, vec![tab])
    }

    fn pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![muxr_core::PaneRegionSnapshot::new(
            muxr_core::PaneId::new(1)?,
            0,
            0,
            2,
            1,
            muxr_core::PaneMouseMode::None,
            0,
        )?])
    }

    fn two_tab_layout() -> rootcause::Result<LayoutSnapshot> {
        LayoutSnapshot::new(
            TabId::new(1)?,
            vec![
                TabSnapshot::new(
                    TabId::new(1)?,
                    "default",
                    PaneId::new(1)?,
                    vec![PaneSnapshot {
                        tracked_process_state: muxr_core::TrackedProcessState::None,
                        cwd: "/tmp/tab-1".to_owned(),
                        cmd_label: None,
                        focus_seq: 1,
                        git_stats: None,
                        id: PaneId::new(1)?,
                        title: "shell".to_owned(),
                    }],
                )?,
                TabSnapshot::new(
                    TabId::new(2)?,
                    "tab 2",
                    PaneId::new(2)?,
                    vec![PaneSnapshot {
                        tracked_process_state: muxr_core::TrackedProcessState::None,
                        cwd: "/tmp/tab-2".to_owned(),
                        cmd_label: None,
                        focus_seq: 1,
                        git_stats: None,
                        id: PaneId::new(2)?,
                        title: "shell".to_owned(),
                    }],
                )?,
            ],
        )
    }

    fn mouse_tracking_pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![muxr_core::PaneRegionSnapshot::new(
            muxr_core::PaneId::new(1)?,
            0,
            0,
            2,
            1,
            muxr_core::PaneMouseMode::ButtonMotion,
            0,
        )?])
    }

    fn split_mouse_tracking_pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![
            muxr_core::PaneRegionSnapshot::new(
                muxr_core::PaneId::new(1)?,
                0,
                0,
                2,
                1,
                muxr_core::PaneMouseMode::ButtonMotion,
                0,
            )?,
            muxr_core::PaneRegionSnapshot::new(
                muxr_core::PaneId::new(2)?,
                2,
                0,
                2,
                1,
                muxr_core::PaneMouseMode::None,
                0,
            )?,
        ])
    }

    fn render_baseline() -> rootcause::Result<muxr_core::RenderBaseline> {
        muxr_core::RenderBaseline::new(
            1,
            TerminalSize::new(2, 1)?,
            muxr_core::RenderCursor {
                row: 0,
                col: 1,
                visible: true,
            },
            vec![muxr_core::RenderRowSpan::new(
                0,
                0,
                vec![self::render_cell("a"), self::render_cell("b")],
            )?],
        )
    }

    fn render_cell(text: &str) -> muxr_core::RenderCell {
        muxr_core::RenderCell::narrow(text, muxr_core::RenderStyle::default())
    }

    #[derive(Default)]
    struct CountingWriter {
        flushes: usize,
    }

    impl std::io::Write for CountingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes = self.flushes.saturating_add(1);
            Ok(())
        }
    }

    fn runtime() -> rootcause::Result<tokio::runtime::Runtime> {
        Ok(tokio::runtime::Runtime::new().context("failed to build muxr client mouse test runtime")?)
    }
}
