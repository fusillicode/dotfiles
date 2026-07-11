use std::collections::HashMap;
use std::collections::HashSet;

use bytes::Bytes;
use bytes::BytesMut;
use compact_str::CompactString;
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
use super::PaneScrollLineMove;
use super::RenderBaseline;
use super::RenderCell;
use super::RenderCellWidth;
use super::RenderCursor;
use super::RenderDiff;
use super::RenderHyperlink;
use super::RenderRowSpan;
use super::RenderStyle;
use super::RenderUpdate;
use super::TabId;
use super::TerminalSize;
use super::pane_render::RenderHyperlinkPresence;
use crate::SessionName;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProtocolFrameKind {
    Domain = 0,
    HyperlinkTableRender = 1,
}

impl TryFrom<u8> for ProtocolFrameKind {
    type Error = rootcause::Report;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Domain),
            1 => Ok(Self::HyperlinkTableRender),
            _ => Err(report!("invalid muxr protocol frame kind").attach(format!("kind={value}"))),
        }
    }
}

/// Owned muxr protocol frame bytes.
///
/// This intentionally wraps [`Bytes`] instead of exposing it directly from the encoder API: muxr keeps a domain-owned
/// protocol type at the core boundary, while transports can still take the `Bytes` buffer without copying the payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolFrame(Bytes);

impl ProtocolFrame {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    #[must_use]
    pub fn into_bytes(self) -> Bytes {
        self.0
    }

    fn from_payload(kind: ProtocolFrameKind, payload: &[u8]) -> Self {
        let mut frame = BytesMut::with_capacity(1_usize.saturating_add(payload.len()));
        frame.extend_from_slice(&[kind as u8]);
        frame.extend_from_slice(payload);
        Self(frame.freeze())
    }
}

impl AsRef<[u8]> for ProtocolFrame {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

/// Frames raw rkyv payload bytes by prepending muxr's frame kind.
impl From<&[u8]> for ProtocolFrame {
    fn from(payload: &[u8]) -> Self {
        Self::from_payload(ProtocolFrameKind::Domain, payload)
    }
}

impl TryFrom<&ClientRequest> for ProtocolFrame {
    type Error = rootcause::Report;

    fn try_from(request: &ClientRequest) -> Result<Self, Self::Error> {
        let payload = rkyv::to_bytes::<rkyv::rancor::Error>(request)
            .map_err(|error| report!("failed to serialize muxr protocol frame").attach(format!("{error:?}")))?;
        Ok(Self::from(payload.as_slice()))
    }
}

impl TryFrom<&ServerEvent> for ProtocolFrame {
    type Error = rootcause::Report;

    fn try_from(event: &ServerEvent) -> Result<Self, Self::Error> {
        if let ServerEvent::Render(update) = event
            && self::render_update_hyperlink_presence(update) == RenderHyperlinkPresence::Present
        {
            let wire_update = HyperlinkTableRenderUpdate::from_domain(update)?;
            let payload = rkyv::to_bytes::<rkyv::rancor::Error>(&wire_update)
                .map_err(|error| report!("failed to serialize muxr protocol frame").attach(format!("{error:?}")))?;
            return Ok(Self::from_payload(
                ProtocolFrameKind::HyperlinkTableRender,
                payload.as_slice(),
            ));
        }

        let payload = rkyv::to_bytes::<rkyv::rancor::Error>(event)
            .map_err(|error| report!("failed to serialize muxr protocol frame").attach(format!("{error:?}")))?;
        Ok(Self::from(payload.as_slice()))
    }
}

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
        movement: PaneScrollLineMove,
    },
    Error(ServerError),
    Detached,
}

#[derive(rkyv::Archive, Debug, rkyv::Deserialize, rkyv::Serialize)]
struct HyperlinkTableRenderUpdate {
    frame: HyperlinkTableRenderFrame,
    hyperlinks: Vec<String>,
}

impl HyperlinkTableRenderUpdate {
    fn from_domain(update: &RenderUpdate) -> rootcause::Result<Self> {
        let mut table = HyperlinkTableBuilder::default();
        let frame = HyperlinkTableRenderFrame::from_domain(update, &mut table)?;
        Ok(Self {
            frame,
            hyperlinks: table.into_uris(),
        })
    }

    fn into_domain(self) -> rootcause::Result<RenderUpdate> {
        let mut hyperlinks = ResolvedHyperlinks::new(self.hyperlinks)?;
        let update = self.frame.into_domain(&mut hyperlinks)?;
        hyperlinks.validate_all_used()?;
        Ok(update)
    }
}

#[derive(rkyv::Archive, Debug, rkyv::Deserialize, rkyv::Serialize)]
enum HyperlinkTableRenderFrame {
    Baseline {
        cursor: RenderCursor,
        rows: Vec<HyperlinkTableRenderRowSpan>,
        seq: u64,
        size: TerminalSize,
    },
    Diff {
        base_seq: u64,
        cursor: RenderCursor,
        rows: Vec<HyperlinkTableRenderRowSpan>,
        seq: u64,
        size: TerminalSize,
    },
}

impl HyperlinkTableRenderFrame {
    fn from_domain(update: &RenderUpdate, table: &mut HyperlinkTableBuilder) -> rootcause::Result<Self> {
        Ok(match update {
            RenderUpdate::Baseline(baseline) => Self::Baseline {
                cursor: baseline.cursor().clone(),
                rows: HyperlinkTableRenderRowSpan::from_domain_rows(baseline.rows(), table)?,
                seq: baseline.seq(),
                size: baseline.size().clone(),
            },
            RenderUpdate::Diff(diff) => Self::Diff {
                base_seq: diff.base_seq(),
                cursor: diff.cursor().clone(),
                rows: HyperlinkTableRenderRowSpan::from_domain_rows(diff.rows(), table)?,
                seq: diff.seq(),
                size: diff.size().clone(),
            },
        })
    }

    fn into_domain(self, hyperlinks: &mut ResolvedHyperlinks) -> rootcause::Result<RenderUpdate> {
        match self {
            Self::Baseline {
                cursor,
                rows,
                seq,
                size,
            } => Ok(RenderUpdate::Baseline(RenderBaseline::new(
                seq,
                size,
                cursor,
                HyperlinkTableRenderRowSpan::into_domain_rows(rows, hyperlinks)?,
            )?)),
            Self::Diff {
                base_seq,
                cursor,
                rows,
                seq,
                size,
            } => Ok(RenderUpdate::Diff(RenderDiff::new(
                base_seq,
                seq,
                size,
                cursor,
                HyperlinkTableRenderRowSpan::into_domain_rows(rows, hyperlinks)?,
            )?)),
        }
    }
}

#[derive(rkyv::Archive, Debug, rkyv::Deserialize, rkyv::Serialize)]
struct HyperlinkTableRenderRowSpan {
    cells: Vec<HyperlinkTableRenderCell>,
    col: u16,
    row: u16,
}

impl HyperlinkTableRenderRowSpan {
    fn from_domain_rows(rows: &[RenderRowSpan], table: &mut HyperlinkTableBuilder) -> rootcause::Result<Vec<Self>> {
        rows.iter().map(|row| Self::from_domain(row, table)).collect()
    }

    fn from_domain(row: &RenderRowSpan, table: &mut HyperlinkTableBuilder) -> rootcause::Result<Self> {
        Ok(Self {
            cells: row
                .cells()
                .iter()
                .map(|cell| HyperlinkTableRenderCell::from_domain(cell, table))
                .collect::<rootcause::Result<Vec<_>>>()?,
            col: row.col(),
            row: row.row(),
        })
    }

    fn into_domain_rows(rows: Vec<Self>, hyperlinks: &mut ResolvedHyperlinks) -> rootcause::Result<Vec<RenderRowSpan>> {
        rows.into_iter().map(|row| row.into_domain(hyperlinks)).collect()
    }

    fn into_domain(self, hyperlinks: &mut ResolvedHyperlinks) -> rootcause::Result<RenderRowSpan> {
        RenderRowSpan::new(
            self.row,
            self.col,
            self.cells
                .into_iter()
                .map(|cell| cell.into_domain(hyperlinks))
                .collect::<rootcause::Result<Vec<_>>>()?,
        )
    }
}

#[derive(rkyv::Archive, Debug, rkyv::Deserialize, rkyv::Serialize)]
struct HyperlinkTableRenderCell {
    hyperlink_id: Option<u32>,
    style: RenderStyle,
    text: CompactString,
    width: RenderCellWidth,
}

impl HyperlinkTableRenderCell {
    fn from_domain(cell: &RenderCell, table: &mut HyperlinkTableBuilder) -> rootcause::Result<Self> {
        Ok(Self {
            hyperlink_id: cell.hyperlink().map(|hyperlink| table.id_for(hyperlink)).transpose()?,
            style: cell.style(),
            text: CompactString::new(cell.text()),
            width: cell.width(),
        })
    }

    fn into_domain(self, hyperlinks: &mut ResolvedHyperlinks) -> rootcause::Result<RenderCell> {
        let mut cell = match self.width {
            RenderCellWidth::Narrow => RenderCell::narrow(&self.text, self.style),
            RenderCellWidth::Wide => RenderCell::wide(&self.text, self.style),
            RenderCellWidth::WideContinuation if self.text.is_empty() => RenderCell::wide_continuation(self.style),
            RenderCellWidth::WideContinuation => {
                return Err(report!("invalid muxr hyperlink-table render cell")
                    .attach("reason=wide continuation must not carry text"));
            }
        };
        if let Some(id) = self.hyperlink_id {
            cell = cell.with_hyperlink(hyperlinks.resolve(id)?);
        }
        Ok(cell)
    }
}

#[derive(Default)]
struct HyperlinkTableBuilder {
    by_uri: HashMap<String, u32>,
    uris: Vec<String>,
}

impl HyperlinkTableBuilder {
    fn id_for(&mut self, hyperlink: &RenderHyperlink) -> rootcause::Result<u32> {
        if let Some(id) = self.by_uri.get(hyperlink.uri()) {
            return Ok(*id);
        }
        let id = u32::try_from(self.uris.len())?
            .checked_add(1)
            .ok_or_else(|| report!("muxr hyperlink table id overflowed"))?;
        let uri = hyperlink.uri().to_owned();
        self.by_uri.insert(uri.clone(), id);
        self.uris.push(uri);
        Ok(id)
    }

    fn into_uris(self) -> Vec<String> {
        self.uris
    }
}

struct ResolvedHyperlinks {
    hyperlinks: Vec<RenderHyperlink>,
    references: Vec<HyperlinkReference>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HyperlinkReference {
    Unreferenced,
    Referenced,
}

impl ResolvedHyperlinks {
    fn new(uris: Vec<String>) -> rootcause::Result<Self> {
        let mut seen = HashSet::with_capacity(uris.len());
        let mut hyperlinks = Vec::with_capacity(uris.len());
        for uri in uris {
            if !seen.insert(uri.clone()) {
                return Err(report!("invalid muxr hyperlink table").attach("reason=duplicate uri"));
            }
            hyperlinks.push(RenderHyperlink::new(uri)?);
        }
        let references = vec![HyperlinkReference::Unreferenced; hyperlinks.len()];
        Ok(Self { hyperlinks, references })
    }

    fn resolve(&mut self, id: u32) -> rootcause::Result<RenderHyperlink> {
        let Some(index) = id.checked_sub(1).and_then(|index| usize::try_from(index).ok()) else {
            return Err(report!("invalid muxr hyperlink table id")
                .attach("reason=id must be nonzero")
                .attach(format!("id={id}")));
        };
        let Some(hyperlink) = self.hyperlinks.get(index) else {
            return Err(report!("invalid muxr hyperlink table id")
                .attach("reason=id is outside hyperlink table")
                .attach(format!("id={id}"))
                .attach(format!("table_len={}", self.hyperlinks.len())));
        };
        let Some(reference) = self.references.get_mut(index) else {
            return Err(report!("invalid muxr hyperlink table bookkeeping"));
        };
        *reference = HyperlinkReference::Referenced;
        Ok(hyperlink.clone())
    }

    fn validate_all_used(&self) -> rootcause::Result<()> {
        if self
            .references
            .iter()
            .all(|reference| *reference == HyperlinkReference::Referenced)
        {
            return Ok(());
        }
        Err(report!("invalid muxr hyperlink table").attach("reason=unused uri"))
    }
}

/// Encode a client request as a muxr protocol frame containing a rkyv payload.
///
/// # Errors
/// - The request cannot be serialized.
pub fn encode_client_request(request: &ClientRequest) -> rootcause::Result<ProtocolFrame> {
    ProtocolFrame::try_from(request)
}

/// Decode a client request from one muxr protocol frame containing a rkyv payload.
///
/// # Errors
/// - The frame is empty or not a valid client request frame.
/// - The decoded request cannot be deserialized into valid domain values.
pub fn decode_client_request(line: &[u8]) -> rootcause::Result<ClientRequest> {
    let payload = self::decode_domain_payload(line)?;
    let archived = rkyv::access::<rkyv::Archived<ClientRequest>, rkyv::rancor::Error>(&payload)
        .map_err(|error| report!("failed to validate muxr protocol frame").attach(format!("{error:?}")))?;
    rkyv::deserialize::<ClientRequest, rkyv::rancor::Error>(archived)
        .map_err(|error| report!("failed to deserialize muxr protocol frame").attach(format!("{error:?}")))
}

/// Encode a server event as a muxr protocol frame containing a rkyv payload.
///
/// # Errors
/// - The event cannot be serialized.
pub fn encode_server_event(event: &ServerEvent) -> rootcause::Result<ProtocolFrame> {
    ProtocolFrame::try_from(event)
}

/// Decode a server event from one muxr protocol frame containing a rkyv payload.
///
/// # Errors
/// - The frame is empty or not a valid server event frame.
/// - The decoded event cannot be deserialized into valid domain values.
pub fn decode_server_event(line: &[u8]) -> rootcause::Result<ServerEvent> {
    let (kind, payload) = self::protocol_payload(line)?;
    let payload = self::align_protocol_payload(payload);
    match kind {
        ProtocolFrameKind::Domain => {
            let archived = rkyv::access::<rkyv::Archived<ServerEvent>, rkyv::rancor::Error>(&payload)
                .map_err(|error| report!("failed to validate muxr protocol frame").attach(format!("{error:?}")))?;
            rkyv::deserialize::<ServerEvent, rkyv::rancor::Error>(archived)
                .map_err(|error| report!("failed to deserialize muxr protocol frame").attach(format!("{error:?}")))
        }
        ProtocolFrameKind::HyperlinkTableRender => {
            let archived = rkyv::access::<rkyv::Archived<HyperlinkTableRenderUpdate>, rkyv::rancor::Error>(&payload)
                .map_err(|error| report!("failed to validate muxr protocol frame").attach(format!("{error:?}")))?;
            let update = rkyv::deserialize::<HyperlinkTableRenderUpdate, rkyv::rancor::Error>(archived)
                .map_err(|error| report!("failed to deserialize muxr protocol frame").attach(format!("{error:?}")))?
                .into_domain()?;
            Ok(ServerEvent::Render(update))
        }
    }
}

fn decode_domain_payload(frame: &[u8]) -> rootcause::Result<AlignedVec> {
    let (kind, payload) = self::protocol_payload(frame)?;
    if kind != ProtocolFrameKind::Domain {
        return Err(report!("invalid muxr protocol frame kind")
            .attach("expected=domain")
            .attach(format!("actual={kind:?}")));
    }
    Ok(self::align_protocol_payload(payload))
}

fn protocol_payload(frame: &[u8]) -> rootcause::Result<(ProtocolFrameKind, &[u8])> {
    let Some((&kind, payload)) = frame.split_first() else {
        return Err(report!("empty muxr protocol frame"));
    };
    let kind = ProtocolFrameKind::try_from(kind)?;
    if payload.is_empty() {
        return Err(report!("empty muxr protocol payload"));
    }
    Ok((kind, payload))
}

fn align_protocol_payload(payload: &[u8]) -> AlignedVec {
    // Socket buffers have arbitrary byte alignment; rkyv checked access requires aligned archived bytes.
    let mut aligned = AlignedVec::with_capacity(payload.len());
    aligned.extend_from_slice(payload);
    aligned
}

const fn render_update_hyperlink_presence(update: &RenderUpdate) -> RenderHyperlinkPresence {
    match update {
        RenderUpdate::Baseline(baseline) => baseline.hyperlink_presence(),
        RenderUpdate::Diff(diff) => diff.hyperlink_presence(),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use test_that::prelude::*;

    use super::super::keyboard_input::ClientKeyCode;
    use super::super::keyboard_input::ClientKeyModifiers;
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
    use super::super::pane_render::RenderCursorShape;
    use super::super::pane_render::RenderCursorVisibility;
    use super::super::pane_render::RenderDiff;
    use super::super::pane_render::RenderRowSpan;
    use super::super::pane_render::RenderStyle;
    use super::super::pane_render::test_helpers as pane_render_test_helpers;
    use super::super::session_attach::AttachRequest;
    use super::super::terminal::TerminalSize;
    use super::super::tracked_process::TrackedProcessState;
    use super::*;

    #[test]
    fn test_protocol_frame_from_when_payload_is_raw_prepends_domain_kind() {
        let frame = ProtocolFrame::from(b"payload".as_slice());
        let expected = b"\0payload";

        assert_that!(frame.as_bytes(), eq(expected));
        assert_that!(AsRef::<[u8]>::as_ref(&frame), eq(expected));
        assert_that!(frame.into_bytes().as_ref(), eq(expected));
    }

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
    #[case::modified_enter_key(ClientRequest::Key(modified_enter_key()))]
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
        assert_that!(
            decode_client_request(encode_client_request(&request)?.as_bytes())?,
            eq(request)
        );
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
            movement: PaneScrollLineMove::Unchanged,
        })]
    #[case::error(ServerEvent::Error(ServerError::unexpected_request(ClientRequest::Detach)))]
    #[case::detached(ServerEvent::Detached)]
    fn test_server_event_codec_when_frame_round_trips_returns_original(
        #[case] event: ServerEvent,
    ) -> rootcause::Result<()> {
        assert_that!(decode_server_event(encode_server_event(&event)?.as_bytes())?, eq(event));
        Ok(())
    }

    #[test]
    fn test_server_event_codec_when_render_update_is_invalid_returns_error() -> rootcause::Result<()> {
        let event = self::invalid_render_event()?;
        let encoded = encode_server_event(&event)?;

        assert_that!(decode_server_event(encoded.as_bytes()), err(anything()));
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

        assert_that!(decode_server_event(encoded.as_bytes()), err(anything()));
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

        assert_that!(decode_server_event(encoded.as_bytes()), err(anything()));
        Ok(())
    }

    #[test]
    fn test_client_request_codec_when_frame_kind_is_invalid_returns_error() {
        let encoded = b"not-muxr-rkyv";

        assert_that!(decode_client_request(encoded), err(anything()));
    }

    #[test]
    fn test_protocol_codec_when_frame_kind_is_unknown_returns_error() -> rootcause::Result<()> {
        let encoded = [u8::MAX, b'x'];

        let Err(error) = decode_server_event(&encoded) else {
            return Err(report!("expected unknown frame kind rejection"));
        };

        assert_that!(
            format!("{error:?}"),
            contains_substring("invalid muxr protocol frame kind")
        );
        Ok(())
    }

    #[test]
    fn test_hyperlink_table_render_when_links_repeat_encodes_deterministically_and_shares_decoded_uri()
    -> rootcause::Result<()> {
        let event = ServerEvent::Render(RenderUpdate::Baseline(linked_render_baseline()?));
        let first = encode_server_event(&event)?;
        let second = encode_server_event(&event)?;

        assert_that!(second.as_bytes(), eq(first.as_bytes()));
        let ServerEvent::Render(RenderUpdate::Baseline(decoded)) = decode_server_event(first.as_bytes())? else {
            return Err(report!("expected decoded render baseline"));
        };
        let row = decoded
            .rows()
            .first()
            .ok_or_else(|| report!("expected decoded render row"))?;
        let first_link = row
            .cells()
            .first()
            .and_then(RenderCell::hyperlink)
            .ok_or_else(|| report!("expected first decoded hyperlink"))?;
        let second_link = row
            .cells()
            .get(1)
            .and_then(RenderCell::hyperlink)
            .ok_or_else(|| report!("expected second decoded hyperlink"))?;
        assert_that!(first_link.uri(), eq("https://example.com"));
        assert_that!(first_link.shares_uri_with(second_link), eq(true));
        Ok(())
    }

    #[rstest]
    #[case::zero_id(vec!["https://example.com"], vec![0, 0])]
    #[case::out_of_range_id(vec!["https://example.com"], vec![2, 2])]
    #[case::duplicate_uri(vec!["https://example.com", "https://example.com"], vec![1, 2])]
    #[case::unused_uri(vec!["https://example.com", "https://unused.example.com"], vec![1, 1])]
    fn test_hyperlink_table_render_when_table_is_noncanonical_returns_error(
        #[case] hyperlinks: Vec<&str>,
        #[case] hyperlink_ids: Vec<u32>,
    ) -> rootcause::Result<()> {
        let wire_update = self::raw_hyperlink_table_render_update(hyperlinks, hyperlink_ids)?;
        let encoded = self::encode_hyperlink_table_render_update(&wire_update)?;

        assert_that!(decode_server_event(encoded.as_bytes()), err(anything()));
        Ok(())
    }

    #[test]
    fn test_hyperlink_table_render_when_url_frame_is_encoded_is_smaller_than_direct_domain_encoding()
    -> rootcause::Result<()> {
        let uri = "https://example.com/muxr/performance/reference";
        let cells = (0..320)
            .map(|_| self::render_cell("x").with_hyperlink_uri(uri))
            .collect::<rootcause::Result<Vec<_>>>()?;
        let event = ServerEvent::Render(RenderUpdate::Baseline(RenderBaseline::new(
            1,
            terminal_size(320, 1)?,
            RenderCursor {
                row: 0,
                col: 0,
                shape: RenderCursorShape::Default,
                visibility: RenderCursorVisibility::Visible,
            },
            vec![RenderRowSpan::new(0, 0, cells)?],
        )?));
        let direct_payload = rkyv::to_bytes::<rkyv::rancor::Error>(&event)
            .map_err(|error| report!("failed to serialize direct comparison frame").attach(format!("{error:?}")))?;

        let table_frame = encode_server_event(&event)?;
        let direct_size = 1_usize.saturating_add(direct_payload.len());

        assert_that!(table_frame.as_bytes().len(), lt(direct_size));
        Ok(())
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

    fn modified_enter_key() -> ClientKey {
        ClientKey {
            code: ClientKeyCode::Enter,
            modifiers: ClientKeyModifiers::SHIFT,
            raw_bytes: b"\x1b[13;2u".to_vec(),
        }
    }

    fn layout_snapshot() -> rootcause::Result<LayoutSnapshot> {
        let active_tab = TabId::new(1)?;
        let active_pane = PaneId::new(1)?;
        let pane = PaneSnapshot {
            tracked_process_state: TrackedProcessState::None,
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
            tracked_process_state: TrackedProcessState::None,
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
                shape: RenderCursorShape::Default,
                visibility: RenderCursorVisibility::Visible,
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
                shape: RenderCursorShape::Default,
                visibility: RenderCursorVisibility::Visible,
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
                shape: RenderCursorShape::Default,
                visibility: RenderCursorVisibility::Visible,
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
                    shape: RenderCursorShape::Default,
                    visibility: RenderCursorVisibility::Visible,
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

    fn raw_hyperlink_table_render_update(
        hyperlinks: Vec<&str>,
        hyperlink_ids: Vec<u32>,
    ) -> rootcause::Result<HyperlinkTableRenderUpdate> {
        let cells = hyperlink_ids
            .into_iter()
            .map(|hyperlink_id| HyperlinkTableRenderCell {
                hyperlink_id: Some(hyperlink_id),
                style: RenderStyle::default(),
                text: CompactString::new("x"),
                width: RenderCellWidth::Narrow,
            })
            .collect::<Vec<_>>();
        Ok(HyperlinkTableRenderUpdate {
            frame: HyperlinkTableRenderFrame::Baseline {
                cursor: RenderCursor {
                    row: 0,
                    col: 0,
                    shape: RenderCursorShape::Default,
                    visibility: RenderCursorVisibility::Visible,
                },
                rows: vec![HyperlinkTableRenderRowSpan { cells, col: 0, row: 0 }],
                seq: 1,
                size: terminal_size(2, 1)?,
            },
            hyperlinks: hyperlinks.into_iter().map(str::to_owned).collect(),
        })
    }

    fn encode_hyperlink_table_render_update(update: &HyperlinkTableRenderUpdate) -> rootcause::Result<ProtocolFrame> {
        let payload = rkyv::to_bytes::<rkyv::rancor::Error>(update)
            .map_err(|error| report!("failed to serialize raw hyperlink-table frame").attach(format!("{error:?}")))?;
        Ok(ProtocolFrame::from_payload(
            ProtocolFrameKind::HyperlinkTableRender,
            payload.as_slice(),
        ))
    }
}
