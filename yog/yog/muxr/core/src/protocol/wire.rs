use rkyv::util::AlignedVec;
use rootcause::report;
use serde::Serialize;

use super::AttachAccepted;
use super::AttachRequest;
use super::ClientKey;
use super::ClientMouseEvent;
use super::ClientMousePosition;
use super::LayoutSnapshot;
use super::PaneRegionsSnapshot;
use super::PaneScrollDirection;
use super::RenderUpdate;
use super::TabId;
use super::TerminalSize;
use crate::SessionName;

const PROTOCOL_FRAME_MAGIC: &[u8; 9] = b"MUXR-RKYV";

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
#[serde(tag = "code", content = "msg", rename_all = "snake_case")]
pub enum ServerError {
    ClientAlreadyAttached,
    SessionMismatch { expected: SessionName, actual: SessionName },
    UnexpectedRequest { request: Box<ClientRequest> },
}

impl ServerError {
    #[must_use]
    pub fn unexpected_request(request: ClientRequest) -> Self {
        Self::UnexpectedRequest {
            request: Box::new(request),
        }
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ClientAlreadyAttached => "client_already_attached",
            Self::SessionMismatch { .. } => "session_mismatch",
            Self::UnexpectedRequest { .. } => "unexpected_request",
        }
    }

    #[must_use]
    pub fn msg(&self) -> String {
        match self {
            Self::ClientAlreadyAttached => "a muxr client is already attached to this session".to_owned(),
            Self::SessionMismatch { expected, actual } => format!("expected session {expected}, got {actual}"),
            Self::UnexpectedRequest { request } => format!("unexpected client request during attach: {request:?}"),
        }
    }
}

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum ClientRequest {
    Attach(AttachRequest),
    DeleteSession,
    Ping,
    Pong,
    Detach,
    RenderResync,
    Resize(TerminalSize),
    Input(Vec<u8>),
    Paste(Vec<u8>),
    Key(ClientKey),
    Mouse(ClientMouseEvent),
    ScrollPaneLineAt {
        position: ClientMousePosition,
        direction: PaneScrollDirection,
    },
    FocusPaneAt(ClientMousePosition),
    FocusTab(TabId),
}

#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum ServerEvent {
    Attached(AttachAccepted),
    Deleted,
    Ping,
    Pong,
    Layout(LayoutSnapshot),
    SidebarLayout(LayoutSnapshot),
    PaneRegions(PaneRegionsSnapshot),
    Render(RenderUpdate),
    ScrollPaneLineResult {
        position: ClientMousePosition,
        direction: PaneScrollDirection,
        scrolled: bool,
    },
    Error(ServerError),
    Detached,
}

/// Encode a client request as a rkyv protocol payload.
///
/// # Errors
/// - The request cannot be serialized.
pub fn encode_client_request(request: &ClientRequest) -> rootcause::Result<Vec<u8>> {
    let payload = rkyv::to_bytes::<rkyv::rancor::Error>(request)
        .map_err(|error| report!("failed to serialize muxr protocol frame").attach(format!("{error:?}")))?;
    Ok(self::encode_protocol_frame(payload.as_slice()))
}

/// Decode a client request from one rkyv protocol payload.
///
/// # Errors
/// - The frame is empty or not a valid client request payload.
/// - The decoded request cannot be deserialized into valid domain values.
pub fn decode_client_request(line: &[u8]) -> rootcause::Result<ClientRequest> {
    let payload = self::decode_protocol_payload(line)?;
    let archived = rkyv::access::<rkyv::Archived<ClientRequest>, rkyv::rancor::Error>(&payload)
        .map_err(|error| report!("failed to validate muxr protocol frame").attach(format!("{error:?}")))?;
    rkyv::deserialize::<ClientRequest, rkyv::rancor::Error>(archived)
        .map_err(|error| report!("failed to deserialize muxr protocol frame").attach(format!("{error:?}")))
}

/// Encode a server event as a rkyv protocol payload.
///
/// # Errors
/// - The event cannot be serialized.
pub fn encode_server_event(event: &ServerEvent) -> rootcause::Result<Vec<u8>> {
    let payload = rkyv::to_bytes::<rkyv::rancor::Error>(event)
        .map_err(|error| report!("failed to serialize muxr protocol frame").attach(format!("{error:?}")))?;
    Ok(self::encode_protocol_frame(payload.as_slice()))
}

/// Decode a server event from one rkyv protocol payload.
///
/// # Errors
/// - The frame is empty or not a valid server event payload.
/// - The decoded event cannot be deserialized into valid domain values.
pub fn decode_server_event(line: &[u8]) -> rootcause::Result<ServerEvent> {
    let payload = self::decode_protocol_payload(line)?;
    let archived = rkyv::access::<rkyv::Archived<ServerEvent>, rkyv::rancor::Error>(&payload)
        .map_err(|error| report!("failed to validate muxr protocol frame").attach(format!("{error:?}")))?;
    rkyv::deserialize::<ServerEvent, rkyv::rancor::Error>(archived)
        .map_err(|error| report!("failed to deserialize muxr protocol frame").attach(format!("{error:?}")))
}

fn encode_protocol_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = PROTOCOL_FRAME_MAGIC.to_vec();
    frame.extend_from_slice(payload);
    frame
}

fn decode_protocol_payload(frame: &[u8]) -> rootcause::Result<AlignedVec> {
    if frame.is_empty() {
        return Err(report!("empty muxr protocol frame"));
    }
    let Some(payload) = frame.strip_prefix(PROTOCOL_FRAME_MAGIC) else {
        return Err(report!("invalid muxr protocol frame")
            .attach("reason=missing rkyv frame magic")
            .attach(format!("magic={PROTOCOL_FRAME_MAGIC:?}")));
    };
    if payload.is_empty() {
        return Err(report!("empty muxr protocol payload"));
    }
    // Socket buffers have arbitrary byte alignment; rkyv checked access requires aligned archived bytes.
    let mut aligned = AlignedVec::with_capacity(payload.len());
    aligned.extend_from_slice(payload);
    Ok(aligned)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::super::keyboard_input::ClientKeyCode;
    use super::super::keyboard_input::ClientKeyModifiers;
    use super::super::pane_agent::PaneAgentState;
    use super::super::pane_layout::PaneId;
    use super::super::pane_layout::PaneMouseMode;
    use super::super::pane_layout::PaneRegionSnapshot;
    use super::super::pane_layout::PaneRegionsSnapshot;
    use super::super::pane_layout::PaneSnapshot;
    use super::super::pane_layout::TabSnapshot;
    use super::super::pane_layout::test_helpers as pane_layout_test_helpers;
    use super::super::pane_mouse::ClientMouseEventPhase;
    use super::super::pane_render::RenderBaseline;
    use super::super::pane_render::RenderCell;
    use super::super::pane_render::RenderCursor;
    use super::super::pane_render::RenderDiff;
    use super::super::pane_render::RenderRowSpan;
    use super::super::pane_render::RenderStyle;
    use super::super::pane_render::test_helpers as pane_render_test_helpers;
    use super::super::session_attach::AttachRequest;
    use super::super::terminal::TerminalSize;
    use super::*;

    #[rstest]
    #[case::attach(ClientRequest::Attach(client_attach_request()?))]
    #[case::delete_session(ClientRequest::DeleteSession)]
    #[case::ping(ClientRequest::Ping)]
    #[case::pong(ClientRequest::Pong)]
    #[case::detach(ClientRequest::Detach)]
    #[case::render_resync(ClientRequest::RenderResync)]
    #[case::resize(ClientRequest::Resize(terminal_size(120, 40)?))]
    #[case::input(ClientRequest::Input(vec![b'a', b'b', b'\n']))]
    #[case::paste(ClientRequest::Paste(vec![b'a', b'\n', b'b', b'\n']))]
    #[case::key(ClientRequest::Key(client_key()))]
    #[case::mouse(ClientRequest::Mouse(ClientMouseEvent {
            button: 0,
            phase: ClientMouseEventPhase::Press,
            position: ClientMousePosition { row: 2, col: 3 },
        }))]
    #[case::scroll_line(ClientRequest::ScrollPaneLineAt {
            position: ClientMousePosition { row: 2, col: 3 },
            direction: PaneScrollDirection::Down,
        })]
    #[case::focus_pane_at(ClientRequest::FocusPaneAt(ClientMousePosition { row: 2, col: 3 }))]
    #[case::focus_tab(ClientRequest::FocusTab(TabId::new(2)?))]
    fn test_client_request_codec_when_frame_round_trips_returns_original(
        #[case] request: ClientRequest,
    ) -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(decode_client_request(&encode_client_request(&request)?)?, request);
        Ok(())
    }

    #[rstest]
    #[case::attached(ServerEvent::Attached(attach_accepted()?))]
    #[case::deleted(ServerEvent::Deleted)]
    #[case::ping(ServerEvent::Ping)]
    #[case::pong(ServerEvent::Pong)]
    #[case::layout(ServerEvent::Layout(layout_snapshot()?))]
    #[case::sidebar_layout(ServerEvent::SidebarLayout(layout_snapshot()?))]
    #[case::pane_regions(ServerEvent::PaneRegions(pane_regions_snapshot()?))]
    #[case::render_baseline(ServerEvent::Render(RenderUpdate::Baseline(render_baseline()?)))]
    #[case::render_linked_baseline(ServerEvent::Render(RenderUpdate::Baseline(linked_render_baseline()?)))]
    #[case::render_diff(ServerEvent::Render(RenderUpdate::Diff(render_diff()?)))]
    #[case::scroll_line_result(ServerEvent::ScrollPaneLineResult {
            position: ClientMousePosition { row: 2, col: 3 },
            direction: PaneScrollDirection::Down,
            scrolled: false,
        })]
    #[case::error(ServerEvent::Error(ServerError::unexpected_request(ClientRequest::Detach)))]
    #[case::detached(ServerEvent::Detached)]
    fn test_server_event_codec_when_frame_round_trips_returns_original(
        #[case] event: ServerEvent,
    ) -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(decode_server_event(&encode_server_event(&event)?)?, event);
        Ok(())
    }

    #[test]
    fn test_server_event_codec_when_render_update_is_invalid_returns_error() -> rootcause::Result<()> {
        let event = self::invalid_render_event()?;
        let encoded = encode_server_event(&event)?;

        assert2::assert!(decode_server_event(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn test_server_event_codec_when_attached_layout_is_invalid_returns_error() -> rootcause::Result<()> {
        let event = ServerEvent::Attached(AttachAccepted {
            layout: pane_layout_test_helpers::raw_layout_snapshot(
                TabId::new(99)?,
                vec![self::tab_snapshot(
                    1,
                    "default",
                    1,
                    vec![self::pane_snapshot(1, "shell")?],
                )?],
            ),
            pane_regions: self::pane_regions_snapshot()?,
        });
        let encoded = encode_server_event(&event)?;

        assert2::assert!(decode_server_event(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn test_server_event_codec_when_layout_event_is_invalid_returns_error() -> rootcause::Result<()> {
        let event = ServerEvent::Layout(pane_layout_test_helpers::raw_layout_snapshot(
            TabId::new(99)?,
            vec![self::tab_snapshot(
                1,
                "default",
                1,
                vec![self::pane_snapshot(1, "shell")?],
            )?],
        ));
        let encoded = encode_server_event(&event)?;

        assert2::assert!(decode_server_event(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn test_client_request_codec_when_frame_magic_is_missing_returns_error() {
        let encoded = b"not-muxr-rkyv";

        assert2::assert!(decode_client_request(encoded).is_err());
    }

    fn client_attach_request() -> rootcause::Result<AttachRequest> {
        Ok(AttachRequest {
            session: "work".parse()?,
            terminal_size: self::terminal_size(80, 24)?,
        })
    }

    fn attach_accepted() -> rootcause::Result<AttachAccepted> {
        Ok(AttachAccepted {
            layout: self::layout_snapshot()?,
            pane_regions: self::pane_regions_snapshot()?,
        })
    }

    fn client_key() -> ClientKey {
        ClientKey {
            code: ClientKeyCode::Char('E'),
            modifiers: ClientKeyModifiers::SHIFT_ALT,
            raw_bytes: vec![b'\x1b', b'E'],
        }
    }

    fn layout_snapshot() -> rootcause::Result<LayoutSnapshot> {
        let active_tab = TabId::new(1)?;
        let active_pane = PaneId::new(1)?;
        let pane = PaneSnapshot {
            agent_state: PaneAgentState::NoAgent,
            cwd: "/tmp".to_owned(),
            cmd_label: None,
            focus_seq: 1,
            id: active_pane,
            title: "shell".to_owned(),
        };
        let tab = TabSnapshot::new(active_tab, "default", active_pane, vec![pane])?;
        LayoutSnapshot::new(active_tab, vec![tab])
    }

    fn pane_regions_snapshot() -> rootcause::Result<PaneRegionsSnapshot> {
        PaneRegionsSnapshot::new(vec![PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            80,
            24,
            PaneMouseMode::None,
            0,
        )?])
    }

    fn tab_snapshot(
        id: u32,
        title: &str,
        active_pane: u32,
        panes: Vec<PaneSnapshot>,
    ) -> rootcause::Result<TabSnapshot> {
        TabSnapshot::new(TabId::new(id)?, title, PaneId::new(active_pane)?, panes)
    }

    fn pane_snapshot(id: u32, title: &str) -> rootcause::Result<PaneSnapshot> {
        Ok(PaneSnapshot {
            agent_state: PaneAgentState::NoAgent,
            cwd: "/tmp".to_owned(),
            cmd_label: None,
            focus_seq: 1,
            id: PaneId::new(id)?,
            title: title.to_owned(),
        })
    }

    fn terminal_size(cols: u16, rows: u16) -> rootcause::Result<TerminalSize> {
        TerminalSize::new(cols, rows)
    }

    fn render_baseline() -> rootcause::Result<RenderBaseline> {
        RenderBaseline::new(
            1,
            self::terminal_size(4, 2)?,
            RenderCursor {
                row: 1,
                col: 2,
                visible: true,
            },
            vec![
                RenderRowSpan::new(
                    0,
                    0,
                    vec![
                        self::render_cell("a"),
                        self::render_cell("b"),
                        self::render_cell("c"),
                        self::render_cell("d"),
                    ],
                )?,
                RenderRowSpan::new(
                    1,
                    0,
                    vec![
                        self::render_cell("e"),
                        self::render_cell("f"),
                        self::render_cell("g"),
                        self::render_cell("h"),
                    ],
                )?,
            ],
        )
    }

    fn linked_render_baseline() -> rootcause::Result<RenderBaseline> {
        RenderBaseline::new(
            1,
            self::terminal_size(4, 2)?,
            RenderCursor {
                row: 1,
                col: 2,
                visible: true,
            },
            vec![
                RenderRowSpan::new(
                    0,
                    0,
                    vec![
                        self::render_cell("a").with_hyperlink_uri("https://example.com")?,
                        self::render_cell("b").with_hyperlink_uri("https://example.com")?,
                        self::render_cell("c"),
                        self::render_cell("d"),
                    ],
                )?,
                RenderRowSpan::new(
                    1,
                    0,
                    vec![
                        self::render_cell("e"),
                        self::render_cell("f"),
                        self::render_cell("g"),
                        self::render_cell("h"),
                    ],
                )?,
            ],
        )
    }

    fn render_diff() -> rootcause::Result<RenderDiff> {
        RenderDiff::new(
            1,
            2,
            self::terminal_size(4, 2)?,
            RenderCursor {
                row: 1,
                col: 3,
                visible: true,
            },
            vec![RenderRowSpan::new(
                1,
                1,
                vec![self::render_cell("x"), self::render_cell("y")],
            )?],
        )
    }

    fn invalid_render_event() -> rootcause::Result<ServerEvent> {
        Ok(ServerEvent::Render(RenderUpdate::Diff(
            pane_render_test_helpers::raw_render_diff(
                1,
                2,
                self::terminal_size(4, 2)?,
                RenderCursor {
                    row: 0,
                    col: 0,
                    visible: true,
                },
                vec![pane_render_test_helpers::raw_render_row_span(
                    0,
                    0,
                    vec![RenderCell::wide_continuation(RenderStyle::default())],
                )],
            ),
        )))
    }

    fn render_cell(text: &str) -> RenderCell {
        RenderCell::narrow(text, RenderStyle::default())
    }
}
