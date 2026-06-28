use std::path::Path;

use bytes::Bytes;
use futures_util::SinkExt;
use futures_util::StreamExt;
use futures_util::stream::SplitSink;
use futures_util::stream::SplitStream;
use interprocess::local_socket::ConnectOptions;
use interprocess::local_socket::GenericFilePath;
use interprocess::local_socket::ListenerOptions;
use interprocess::local_socket::ToFsName as _;
use interprocess::local_socket::tokio::Listener as LocalSocketListener;
use interprocess::local_socket::tokio::Stream as LocalSocketStream;
use interprocess::local_socket::traits::tokio::Listener as _;
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
use tokio_util::codec::Framed;
use tokio_util::codec::LengthDelimitedCodec;

type FramedSocket = Framed<LocalSocketStream, LengthDelimitedCodec>;
type FrameReader = SplitStream<FramedSocket>;
type FrameWriter = SplitSink<FramedSocket, Bytes>;

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
        let (writer, reader) = frame_socket(stream).split();
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
    writer: FrameWriter,
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
    writer: FrameWriter,
}

impl ServerConnection {
    #[must_use]
    fn from_stream(stream: LocalSocketStream) -> Self {
        let (writer, reader) = frame_socket(stream).split();
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
    writer: FrameWriter,
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

fn frame_socket(stream: LocalSocketStream) -> FramedSocket {
    Framed::new(stream, LengthDelimitedCodec::new())
}

async fn recv_client_request_frame<T>(
    reader: &mut SplitStream<Framed<T, LengthDelimitedCodec>>,
) -> rootcause::Result<Option<ClientRequest>>
where
    T: AsyncRead + AsyncWrite + Send + Unpin,
{
    let Some(frame) = recv_frame(reader).await? else {
        return Ok(None);
    };

    Ok(Some(decode_client_request(&frame)?))
}

async fn recv_server_event_frame<T>(
    reader: &mut SplitStream<Framed<T, LengthDelimitedCodec>>,
) -> rootcause::Result<Option<ServerEvent>>
where
    T: AsyncRead + AsyncWrite + Send + Unpin,
{
    let Some(frame) = recv_frame(reader).await? else {
        return Ok(None);
    };

    Ok(Some(decode_server_event(&frame)?))
}

async fn recv_frame<T>(reader: &mut SplitStream<Framed<T, LengthDelimitedCodec>>) -> rootcause::Result<Option<Bytes>>
where
    T: AsyncRead + AsyncWrite + Send + Unpin,
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

async fn send_client_request_frame<T>(
    writer: &mut SplitSink<Framed<T, LengthDelimitedCodec>, Bytes>,
    request: &ClientRequest,
) -> rootcause::Result<()>
where
    T: AsyncRead + AsyncWrite + Send + Unpin,
{
    send_frame(writer, encode_client_request(request)?.into_bytes()).await
}

async fn send_server_event_frame<T>(
    writer: &mut SplitSink<Framed<T, LengthDelimitedCodec>, Bytes>,
    event: &ServerEvent,
) -> rootcause::Result<()>
where
    T: AsyncRead + AsyncWrite + Send + Unpin,
{
    send_frame(writer, encode_server_event(event)?.into_bytes()).await
}

async fn send_frame<T>(
    writer: &mut SplitSink<Framed<T, LengthDelimitedCodec>, Bytes>,
    frame: Bytes,
) -> rootcause::Result<()>
where
    T: AsyncRead + AsyncWrite + Send + Unpin,
{
    writer
        .send(frame)
        .await
        .context("failed to write muxr transport frame")?;
    Ok(())
}

#[cfg(test)]
mod tests {
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
            let (mut client_writer, _client_reader) = Framed::new(client, LengthDelimitedCodec::new()).split();
            let (_server_writer, mut server_reader) = Framed::new(server, LengthDelimitedCodec::new()).split();

            send_client_request_frame(&mut client_writer, &request).await?;

            pretty_assertions::assert_eq!(recv_client_request_frame(&mut server_reader).await?, Some(request));
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
            let (_client_writer, mut client_reader) = Framed::new(client, LengthDelimitedCodec::new()).split();
            let (mut server_writer, _server_reader) = Framed::new(server, LengthDelimitedCodec::new()).split();

            send_server_event_frame(&mut server_writer, &event).await?;

            pretty_assertions::assert_eq!(recv_server_event_frame(&mut client_reader).await?, Some(event));
            Ok(())
        })
    }

    #[test]
    fn test_transport_when_invalid_frame_is_received_returns_error() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let (client, server) = tokio::io::duplex(1024);
            let (mut client_writer, _client_reader) = Framed::new(client, LengthDelimitedCodec::new()).split();
            let (_server_writer, mut server_reader) = Framed::new(server, LengthDelimitedCodec::new()).split();

            send_frame(&mut client_writer, Bytes::from_static(b"MUXR-BINV")).await?;

            assert2::assert!(recv_client_request_frame(&mut server_reader).await.is_err());
            Ok(())
        })
    }

    #[test]
    fn test_transport_when_empty_payload_is_received_returns_error() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let (client, server) = tokio::io::duplex(1024);
            let (mut client_writer, _client_reader) = Framed::new(client, LengthDelimitedCodec::new()).split();
            let (_server_writer, mut server_reader) = Framed::new(server, LengthDelimitedCodec::new()).split();

            send_frame(&mut client_writer, Bytes::from_static(b"MUXR-RKYV")).await?;

            assert2::assert!(recv_client_request_frame(&mut server_reader).await.is_err());
            Ok(())
        })
    }

    #[test]
    fn test_transport_when_multiple_messages_are_sent_reads_each_frame() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport test runtime")?;

        runtime.block_on(async {
            let (client, server) = tokio::io::duplex(1024);
            let (mut client_writer, _client_reader) = Framed::new(client, LengthDelimitedCodec::new()).split();
            let (_server_writer, mut server_reader) = Framed::new(server, LengthDelimitedCodec::new()).split();
            let resize = TerminalSize::new(120, 40)?;

            send_client_request_frame(&mut client_writer, &ClientRequest::Ping).await?;
            send_client_request_frame(&mut client_writer, &ClientRequest::Resize(resize.clone())).await?;

            pretty_assertions::assert_eq!(
                recv_client_request_frame(&mut server_reader).await?,
                Some(ClientRequest::Ping)
            );
            pretty_assertions::assert_eq!(
                recv_client_request_frame(&mut server_reader).await?,
                Some(ClientRequest::Resize(resize)),
            );
            Ok(())
        })
    }

    #[test]
    fn test_server_listener_bind_when_socket_path_is_too_long_returns_clear_error() {
        let path = Path::new("/").join("x".repeat(200));

        let error = ServerListener::bind(&path).map_or_else(|error| error, |_| report!("expected path length error"));

        assert2::assert!(error.to_string().contains("muxr socket path is too long"));
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
}
