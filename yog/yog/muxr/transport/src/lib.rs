use std::io;
use std::io::IoSlice;
use std::path::Path;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use bytes::Bytes;
use futures_util::StreamExt;
use interprocess::local_socket::ConnectOptions;
use interprocess::local_socket::GenericFilePath;
use interprocess::local_socket::ListenerOptions;
use interprocess::local_socket::ToFsName as _;
use interprocess::local_socket::tokio::Listener as LocalSocketListener;
use interprocess::local_socket::tokio::RecvHalf as LocalSocketRecvHalf;
use interprocess::local_socket::tokio::SendHalf as LocalSocketSendHalf;
use interprocess::local_socket::tokio::Stream as LocalSocketStream;
use interprocess::local_socket::traits::tokio::Listener as _;
use interprocess::local_socket::traits::tokio::Stream as _;
use muxr_core::ClientRequest;
use muxr_core::ServerEvent;
use muxr_core::decode_client_request;
use muxr_core::decode_server_event;
use muxr_core::encode_client_request;
use muxr_core::encode_server_event;
use muxr_core::validate_socket_path;
use rootcause::prelude::ResultExt;
use rootcause::report;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::FramedRead;
use tokio_util::codec::LengthDelimitedCodec;

#[cfg(feature = "benchmarking")]
#[doc(hidden)]
pub mod benchmark_support;

const FRAME_HEADER_LENGTH: usize = size_of::<u32>();
const MAX_FRAME_LENGTH: usize = 8 * 1024 * 1024;

type FrameReader = FramedRead<LocalSocketRecvHalf, LengthDelimitedCodec>;
type LocalFrameWriter = FrameWriter<LocalSocketWriter>;

pub struct ClientConnection {
    reader: ClientEventReader,
    writer: ClientRequestWriter,
}

impl ClientConnection {
    /// Connect to a muxr server socket.
    ///
    /// # Errors
    /// - The local socket name cannot be built.
    /// - The socket connection fails.
    pub async fn connect(path: &Path) -> rootcause::Result<Self> {
        validate_socket_path(path)?;
        let name = path
            .to_fs_name::<GenericFilePath>()
            .context("failed to build muxr socket name")?;
        let stream = ConnectOptions::new()
            .name(name)
            .connect_tokio()
            .await
            .context("failed to connect muxr session socket")?;

        Ok(Self::from_stream(stream))
    }

    #[must_use]
    fn from_stream(stream: LocalSocketStream) -> Self {
        let (writer, reader) = frame_socket(stream);
        Self {
            reader: ClientEventReader { reader },
            writer: ClientRequestWriter { writer },
        }
    }

    /// Split this client connection into independent event reader and request writer halves.
    #[must_use]
    pub fn split(self) -> (ClientEventReader, ClientRequestWriter) {
        (self.reader, self.writer)
    }

    /// Receive one server event.
    ///
    /// # Errors
    /// - The frame cannot be read or decoded.
    pub async fn recv_event(&mut self) -> rootcause::Result<Option<ServerEvent>> {
        self.reader.recv_event().await
    }

    /// Send one client request.
    ///
    /// # Errors
    /// - The request cannot be encoded or written.
    pub async fn send_request(&mut self, request: &ClientRequest) -> rootcause::Result<()> {
        self.writer.send_request(request).await
    }
}

pub struct ClientEventReader {
    reader: FrameReader,
}

impl ClientEventReader {
    /// Receive one server event.
    ///
    /// # Errors
    /// - The frame cannot be read or decoded.
    pub async fn recv_event(&mut self) -> rootcause::Result<Option<ServerEvent>> {
        recv_server_event_frame(&mut self.reader).await
    }
}

pub struct ClientRequestWriter {
    writer: LocalFrameWriter,
}

impl ClientRequestWriter {
    /// Send one client request.
    ///
    /// # Errors
    /// - The request cannot be encoded or written.
    pub async fn send_request(&mut self, request: &ClientRequest) -> rootcause::Result<()> {
        send_client_request_frame(&mut self.writer, request).await
    }
}

pub struct ServerConnection {
    reader: FrameReader,
    writer: LocalFrameWriter,
}

impl ServerConnection {
    #[must_use]
    fn from_stream(stream: LocalSocketStream) -> Self {
        let (writer, reader) = frame_socket(stream);
        Self { reader, writer }
    }

    /// Split this server connection into independent request reader and event writer halves.
    #[must_use]
    pub fn split(self) -> (ServerRequestReader, ServerEventWriter) {
        (
            ServerRequestReader { reader: self.reader },
            ServerEventWriter { writer: self.writer },
        )
    }

    /// Receive one client request.
    ///
    /// # Errors
    /// - The frame cannot be read or decoded.
    pub async fn recv_request(&mut self) -> rootcause::Result<Option<ClientRequest>> {
        recv_client_request_frame(&mut self.reader).await
    }

    /// Send one server event.
    ///
    /// # Errors
    /// - The event cannot be encoded or written.
    pub async fn send_event(&mut self, event: &ServerEvent) -> rootcause::Result<()> {
        send_server_event_frame(&mut self.writer, event).await
    }
}

pub struct ServerRequestReader {
    reader: FrameReader,
}

impl ServerRequestReader {
    /// Receive one client request.
    ///
    /// # Errors
    /// - The frame cannot be read or decoded.
    pub async fn recv_request(&mut self) -> rootcause::Result<Option<ClientRequest>> {
        recv_client_request_frame(&mut self.reader).await
    }
}

pub struct ServerEventWriter {
    writer: LocalFrameWriter,
}

impl ServerEventWriter {
    /// Send one server event.
    ///
    /// # Errors
    /// - The event cannot be encoded or written.
    pub async fn send_event(&mut self, event: &ServerEvent) -> rootcause::Result<()> {
        send_server_event_frame(&mut self.writer, event).await
    }
}

pub struct ServerListener {
    listener: LocalSocketListener,
}

impl ServerListener {
    /// Bind a muxr server listener.
    ///
    /// # Errors
    /// - The local socket name cannot be built.
    /// - The socket cannot be bound.
    pub fn bind(path: &Path) -> rootcause::Result<Self> {
        validate_socket_path(path)?;
        let name = path
            .to_fs_name::<GenericFilePath>()
            .context("failed to build muxr socket name")?;
        let listener = ListenerOptions::new()
            .name(name)
            .create_tokio()
            .context("failed to bind muxr session socket")?;

        Ok(Self { listener })
    }

    /// Accept one muxr client connection.
    ///
    /// # Errors
    /// - The listener cannot accept a client stream.
    pub async fn accept(&self) -> rootcause::Result<ServerConnection> {
        Ok(ServerConnection::from_stream(
            self.listener
                .accept()
                .await
                .context("failed to accept muxr client connection")?,
        ))
    }
}

fn frame_socket(stream: LocalSocketStream) -> (LocalFrameWriter, FrameReader) {
    let (reader, writer) = stream.split();
    (
        FrameWriter::new(LocalSocketWriter(writer)),
        FramedRead::new(reader, frame_codec()),
    )
}

fn frame_codec() -> LengthDelimitedCodec {
    let mut codec = LengthDelimitedCodec::new();
    codec.set_max_frame_length(MAX_FRAME_LENGTH);
    codec
}

async fn recv_client_request_frame<T>(
    reader: &mut FramedRead<T, LengthDelimitedCodec>,
) -> rootcause::Result<Option<ClientRequest>>
where
    T: AsyncRead + Send + Unpin,
{
    let Some(frame) = recv_frame(reader).await? else {
        return Ok(None);
    };

    Ok(Some(decode_client_request(&frame)?))
}

async fn recv_server_event_frame<T>(
    reader: &mut FramedRead<T, LengthDelimitedCodec>,
) -> rootcause::Result<Option<ServerEvent>>
where
    T: AsyncRead + Send + Unpin,
{
    let Some(frame) = recv_frame(reader).await? else {
        return Ok(None);
    };

    Ok(Some(decode_server_event(&frame)?))
}

async fn recv_frame<T>(reader: &mut FramedRead<T, LengthDelimitedCodec>) -> rootcause::Result<Option<Bytes>>
where
    T: AsyncRead + Send + Unpin,
{
    let Some(frame) = reader
        .next()
        .await
        .transpose()
        .context("failed to read muxr transport frame")?
    else {
        return Ok(None);
    };

    if frame.is_empty() {
        return Err(report!("empty muxr transport frame"));
    }

    Ok(Some(frame.freeze()))
}

async fn send_client_request_frame<T>(writer: &mut FrameWriter<T>, request: &ClientRequest) -> rootcause::Result<()>
where
    T: AsyncWrite + Send + Unpin,
{
    send_frame(writer, encode_client_request(request)?.into_bytes()).await
}

async fn send_server_event_frame<T>(writer: &mut FrameWriter<T>, event: &ServerEvent) -> rootcause::Result<()>
where
    T: AsyncWrite + Send + Unpin,
{
    send_frame(writer, encode_server_event(event)?.into_bytes()).await
}

async fn send_frame<T>(writer: &mut FrameWriter<T>, frame: Bytes) -> rootcause::Result<()>
where
    T: AsyncWrite + Send + Unpin,
{
    writer
        .send(frame)
        .await
        .context("failed to write muxr transport frame")?;
    Ok(())
}

struct FrameWriter<T> {
    writer: T,
    pending: Option<PendingFrame>,
}

impl<T> FrameWriter<T>
where
    T: AsyncWrite + Unpin,
{
    const fn new(writer: T) -> Self {
        Self { writer, pending: None }
    }

    async fn send(&mut self, frame: Bytes) -> io::Result<()> {
        // A cancelled send keeps its owned frame and byte offset here, so the next send finishes it before framing
        // another payload. This prevents timeout cancellation from corrupting the byte stream.
        self.write_pending().await?;
        if frame.len() > MAX_FRAME_LENGTH {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame size too big"));
        }
        self.pending = Some(PendingFrame::new(frame)?);
        self.write_pending().await
    }

    async fn write_pending(&mut self) -> io::Result<()> {
        while let Some(pending) = &mut self.pending {
            let written = if pending.written < FRAME_HEADER_LENGTH {
                let Some(header) = pending.header.get(pending.written..) else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid muxr frame write offset",
                    ));
                };
                let slices = [IoSlice::new(header), IoSlice::new(&pending.payload)];
                self.writer.write_vectored(&slices).await?
            } else {
                let payload_offset = pending.written.saturating_sub(FRAME_HEADER_LENGTH);
                let Some(payload) = pending.payload.get(payload_offset..) else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid muxr frame write offset",
                    ));
                };
                let slices = [IoSlice::new(payload)];
                self.writer.write_vectored(&slices).await?
            };
            if written == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write muxr transport frame",
                ));
            }
            pending.written = pending
                .written
                .checked_add(written)
                .filter(|total| *total <= pending.total_len())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid muxr frame write count"))?;
            if pending.written == pending.total_len() {
                self.pending = None;
            }
        }
        Ok(())
    }
}

struct PendingFrame {
    header: [u8; FRAME_HEADER_LENGTH],
    payload: Bytes,
    written: usize,
}

impl PendingFrame {
    fn new(payload: Bytes) -> io::Result<Self> {
        let length = u32::try_from(payload.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame size too big"))?;
        Ok(Self {
            header: length.to_be_bytes(),
            payload,
            written: 0,
        })
    }

    const fn total_len(&self) -> usize {
        FRAME_HEADER_LENGTH.saturating_add(self.payload.len())
    }
}

struct LocalSocketWriter(LocalSocketSendHalf);

impl AsyncWrite for LocalSocketWriter {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buffer: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buffer)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match &mut self.get_mut().0 {
            #[cfg(windows)]
            LocalSocketSendHalf::NamedPipe(writer) => Pin::new(writer).poll_write_vectored(cx, buffers),
            #[cfg(unix)]
            LocalSocketSendHalf::UdSocket(writer) => Pin::new(writer).poll_write_vectored(cx, buffers),
        }
    }

    fn is_write_vectored(&self) -> bool {
        match &self.0 {
            #[cfg(windows)]
            LocalSocketSendHalf::NamedPipe(writer) => writer.is_write_vectored(),
            #[cfg(unix)]
            LocalSocketSendHalf::UdSocket(writer) => writer.is_write_vectored(),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future as _;
    use std::path::Path;

    use muxr_core::AttachAccepted;
    use muxr_core::ClientKey;
    use muxr_core::ClientKeyCode;
    use muxr_core::ClientKeyModifiers;
    use muxr_core::LayoutSnapshot;
    use muxr_core::PaneId;
    use muxr_core::PaneSnapshot;
    use muxr_core::TabId;
    use muxr_core::TabSnapshot;
    use muxr_core::TerminalSize;
    use rstest::rstest;
    use test_that::prelude::*;

    use super::*;

    #[rstest]
    #[case::ping(ClientRequest::Ping)]
    #[case::pong(ClientRequest::Pong)]
    #[case::detach(ClientRequest::Detach)]
    #[case::key(ClientRequest::Key(ClientKey { code: ClientKeyCode::Char('E'), modifiers: ClientKeyModifiers::SHIFT_ALT, raw_bytes: b"\x1bE".to_vec() }))]
    fn test_transport_when_client_request_round_trips_returns_original(
        #[case] request: ClientRequest,
    ) -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let (client, server) = tokio::io::duplex(1024);
            let (mut client_writer, _client_reader) = test_frame_socket(client);
            let (_server_writer, mut server_reader) = test_frame_socket(server);

            send_client_request_frame(&mut client_writer, &request).await?;

            assert_that!(recv_client_request_frame(&mut server_reader).await?, eq(Some(request)));
            Ok(())
        })
    }

    #[rstest]
    #[case::ping(ServerEvent::Ping)]
    #[case::pong(ServerEvent::Pong)]
    #[case::attached(ServerEvent::Attached(AttachAccepted {
        layout: layout_snapshot()?,
        pane_regions: pane_regions_snapshot()?,
    }))]
    #[case::layout(ServerEvent::Layout(layout_snapshot()?))]
    #[case::sidebar_layout(ServerEvent::SidebarLayout(layout_snapshot()?))]
    #[case::pane_regions(ServerEvent::PaneRegions(pane_regions_snapshot()?))]
    fn test_transport_when_server_event_round_trips_returns_original(
        #[case] event: ServerEvent,
    ) -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let (client, server) = tokio::io::duplex(1024);
            let (_client_writer, mut client_reader) = test_frame_socket(client);
            let (mut server_writer, _server_reader) = test_frame_socket(server);

            send_server_event_frame(&mut server_writer, &event).await?;

            assert_that!(recv_server_event_frame(&mut client_reader).await?, eq(Some(event)));
            Ok(())
        })
    }

    #[test]
    fn test_transport_when_invalid_frame_is_received_returns_error() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let (client, server) = tokio::io::duplex(1024);
            let (mut client_writer, _client_reader) = test_frame_socket(client);
            let (_server_writer, mut server_reader) = test_frame_socket(server);

            send_frame(&mut client_writer, Bytes::from_static(b"MUXR-BINV")).await?;

            assert_that!(recv_client_request_frame(&mut server_reader).await, err(anything()));
            Ok(())
        })
    }

    #[test]
    fn test_transport_when_empty_payload_is_received_returns_error() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let (client, server) = tokio::io::duplex(1024);
            let (mut client_writer, _client_reader) = test_frame_socket(client);
            let (_server_writer, mut server_reader) = test_frame_socket(server);

            send_frame(&mut client_writer, Bytes::from_static(b"MUXR-RKYV")).await?;

            assert_that!(recv_client_request_frame(&mut server_reader).await, err(anything()));
            Ok(())
        })
    }

    #[test]
    fn test_transport_when_multiple_messages_are_sent_reads_each_frame() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let (client, server) = tokio::io::duplex(1024);
            let (mut client_writer, _client_reader) = test_frame_socket(client);
            let (_server_writer, mut server_reader) = test_frame_socket(server);
            let resize = TerminalSize::new(120, 40)?;

            send_client_request_frame(&mut client_writer, &ClientRequest::Ping).await?;
            send_client_request_frame(&mut client_writer, &ClientRequest::Resize(resize.clone())).await?;

            assert_that!(
                recv_client_request_frame(&mut server_reader).await?,
                eq(Some(ClientRequest::Ping))
            );
            assert_that!(
                recv_client_request_frame(&mut server_reader).await?,
                eq(Some(ClientRequest::Resize(resize)))
            );
            Ok(())
        })
    }

    #[test]
    fn test_frame_writer_when_vectored_writes_are_partial_writes_one_complete_frame() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let payload = Bytes::from_static(b"partial payload");
            let mut writer = FrameWriter::new(PartialWriter::new(6, None));

            writer.send(payload.clone()).await?;

            let expected = framed_bytes(&payload);
            assert_that!(writer.writer.bytes, eq(expected));
            assert_that!(writer.writer.write_lengths.first(), eq(Some(&6)));
            assert_that!(writer.writer.vectored_writes > 1, eq(true));
            Ok(())
        })
    }

    #[test]
    fn test_frame_writer_when_send_is_cancelled_resumes_frame_before_next_frame() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let first = Bytes::from_static(b"first frame");
            let second = Bytes::from_static(b"second frame");
            let mut writer = FrameWriter::new(PartialWriter::new(6, Some(1)));

            {
                let future = writer.send(first.clone());
                tokio::pin!(future);
                let mut context = Context::from_waker(std::task::Waker::noop());
                assert_that!(matches!(future.as_mut().poll(&mut context), Poll::Pending), eq(true));
            }
            assert_that!(writer.writer.bytes, eq(framed_bytes(&first)[..6].to_vec()));
            assert_that!(writer.writer.write_lengths, eq(vec![6]));

            writer.writer.writes_before_pending = None;
            writer.send(second.clone()).await?;

            let mut expected = framed_bytes(&first);
            expected.extend_from_slice(&framed_bytes(&second));
            assert_that!(writer.writer.bytes, eq(expected));
            Ok(())
        })
    }

    #[test]
    fn test_frame_writer_when_payload_is_sent_borrows_original_buffer() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let payload = Bytes::from(vec![7; 32]);
            let payload_pointer = payload.as_ptr();
            let mut writer = FrameWriter::new(PayloadIdentityWriter::new(payload_pointer, payload.len()));

            writer.send(payload).await?;

            assert_that!(writer.writer.saw_original_payload, eq(true));
            Ok(())
        })
    }

    #[test]
    fn test_frame_writer_when_frame_exceeds_limit_returns_error_without_writing() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let mut writer = FrameWriter::new(PartialWriter::new(usize::MAX, None));

            assert_that!(
                writer.send(Bytes::from(vec![0; MAX_FRAME_LENGTH + 1])).await,
                err(anything())
            );
            assert_that!(writer.writer.bytes, eq(Vec::<u8>::new()));
            Ok(())
        })
    }

    #[test]
    fn test_frame_writer_when_cancelled_frame_precedes_oversized_frame_finishes_cancelled_frame_first()
    -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let first = Bytes::from_static(b"cancelled frame");
            let mut writer = FrameWriter::new(PartialWriter::new(3, Some(1)));

            {
                let future = writer.send(first.clone());
                tokio::pin!(future);
                let mut context = Context::from_waker(std::task::Waker::noop());
                assert_that!(matches!(future.as_mut().poll(&mut context), Poll::Pending), eq(true));
            }

            writer.writer.writes_before_pending = None;
            assert_that!(
                writer.send(Bytes::from(vec![0; MAX_FRAME_LENGTH + 1])).await,
                err(anything())
            );
            assert_that!(writer.writer.bytes, eq(framed_bytes(&first)));
            Ok(())
        })
    }

    #[test]
    fn test_server_listener_bind_when_socket_path_is_too_long_returns_clear_error() {
        let path = Path::new("/").join("x".repeat(200));

        let error = ServerListener::bind(&path).map_or_else(|error| error, |_| report!("expected path length error"));

        assert_that!(error.to_string(), contains_substring("muxr socket path is too long"));
    }

    fn layout_snapshot() -> rootcause::Result<LayoutSnapshot> {
        let active_tab = TabId::new(1)?;
        let active_pane = PaneId::new(1)?;
        let pane = PaneSnapshot {
            tracked_process_state: muxr_core::TrackedProcessState::None,
            cwd: "/tmp".to_owned(),
            cmd_label: None,
            focus_seq: 1,
            id: active_pane,
            title: "shell".to_owned(),
        };
        let tab = TabSnapshot::new(active_tab, "default", active_pane, vec![pane])?;
        LayoutSnapshot::new(active_tab, vec![tab])
    }

    fn pane_regions_snapshot() -> rootcause::Result<muxr_core::PaneRegionsSnapshot> {
        muxr_core::PaneRegionsSnapshot::new(vec![muxr_core::PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            80,
            24,
            muxr_core::PaneMouseMode::None,
            0,
        )?])
    }

    fn test_frame_socket(
        stream: tokio::io::DuplexStream,
    ) -> (
        FrameWriter<tokio::io::WriteHalf<tokio::io::DuplexStream>>,
        FramedRead<tokio::io::ReadHalf<tokio::io::DuplexStream>, LengthDelimitedCodec>,
    ) {
        let (reader, writer) = tokio::io::split(stream);
        (FrameWriter::new(writer), FramedRead::new(reader, frame_codec()))
    }

    fn framed_bytes(payload: &[u8]) -> Vec<u8> {
        let mut frame = u32::try_from(payload.len()).unwrap().to_be_bytes().to_vec();
        frame.extend_from_slice(payload);
        frame
    }

    struct PartialWriter {
        bytes: Vec<u8>,
        max_write: usize,
        write_lengths: Vec<usize>,
        writes_before_pending: Option<usize>,
        vectored_writes: usize,
    }

    struct PayloadIdentityWriter {
        expected_payload_pointer: *const u8,
        expected_payload_length: usize,
        saw_original_payload: bool,
    }

    impl PayloadIdentityWriter {
        const fn new(expected_payload_pointer: *const u8, expected_payload_length: usize) -> Self {
            Self {
                expected_payload_length,
                expected_payload_pointer,
                saw_original_payload: false,
            }
        }
    }

    impl AsyncWrite for PayloadIdentityWriter {
        fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buffer: &[u8]) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(buffer.len()))
        }

        fn poll_write_vectored(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buffers: &[IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            let writer = self.get_mut();
            writer.saw_original_payload |= buffers.iter().any(|buffer| {
                buffer.as_ptr() == writer.expected_payload_pointer && buffer.len() == writer.expected_payload_length
            });
            Poll::Ready(Ok(buffers.iter().map(|buffer| buffer.len()).sum()))
        }

        fn is_write_vectored(&self) -> bool {
            true
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl PartialWriter {
        fn new(max_write: usize, writes_before_pending: Option<usize>) -> Self {
            Self {
                bytes: Vec::new(),
                max_write,
                write_lengths: Vec::new(),
                writes_before_pending,
                vectored_writes: 0,
            }
        }

        fn poll_write_slices(&mut self, buffers: &[IoSlice<'_>]) -> Poll<io::Result<usize>> {
            if self.writes_before_pending == Some(0) {
                return Poll::Pending;
            }
            if let Some(writes) = &mut self.writes_before_pending {
                *writes = writes.saturating_sub(1);
            }

            let mut remaining = self.max_write;
            let initial_len = self.bytes.len();
            for buffer in buffers {
                let count = remaining.min(buffer.len());
                self.bytes.extend_from_slice(&buffer[..count]);
                remaining = remaining.saturating_sub(count);
                if remaining == 0 {
                    break;
                }
            }
            let written = self.bytes.len().saturating_sub(initial_len);
            self.write_lengths.push(written);
            Poll::Ready(Ok(written))
        }
    }

    impl AsyncWrite for PartialWriter {
        fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buffer: &[u8]) -> Poll<io::Result<usize>> {
            self.get_mut().poll_write_slices(&[IoSlice::new(buffer)])
        }

        fn poll_write_vectored(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buffers: &[IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            let writer = self.get_mut();
            writer.vectored_writes = writer.vectored_writes.saturating_add(1);
            writer.poll_write_slices(buffers)
        }

        fn is_write_vectored(&self) -> bool {
            true
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
}
