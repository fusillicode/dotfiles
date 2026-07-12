//! Feature-gated A/B measurement support for the muxr transport send path.

use std::io;
use std::io::IoSlice;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;

use bytes::Bytes;
use futures_util::SinkExt;
use futures_util::StreamExt;
use futures_util::stream::SplitSink;
use futures_util::stream::SplitStream;
use muxr_core::ProtocolFrame;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::ReadBuf;
use tokio_util::codec::Framed;
use tokio_util::codec::LengthDelimitedCodec;

use crate::FrameWriter;

type CopyingFramed<T> = Framed<T, LengthDelimitedCodec>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendPathMode {
    CopyingCodec,
    Vectored,
}

impl SendPathMode {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CopyingCodec => "copying_codec",
            Self::Vectored => "vectored",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SendPathResult {
    pub frames: u64,
    pub wire_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SendPathProbeResult {
    pub frames: u64,
    pub payload_bytes_copied: u64,
    pub payload_copies: u64,
    pub wire_bytes: u64,
}

pub struct SendPathBenchmark {
    state: SendPathState<DiscardIo>,
}

impl SendPathBenchmark {
    #[must_use]
    pub fn new(mode: SendPathMode) -> Self {
        Self {
            state: SendPathState::new(mode, DiscardIo),
        }
    }

    /// Send every prepared payload through the selected complete framing writer.
    ///
    /// # Errors
    /// Returns an error if either writer rejects a frame.
    pub async fn run(&mut self, frames: PreparedSendFrames) -> rootcause::Result<SendPathResult> {
        self::run_frames(&mut self.state, frames).await
    }
}

pub struct PreparedSendFrames(Vec<ProtocolFrame>);

impl PreparedSendFrames {
    #[must_use]
    pub const fn new(frames: Vec<ProtocolFrame>) -> Self {
        Self(frames)
    }
}

/// Observe wire shape and payload ownership outside Criterion timing.
///
/// # Errors
/// Returns an error if the selected writer rejects a frame, emits an unexpected write shape, or does not write every
/// prepared frame.
pub async fn probe_send_path(frames: PreparedSendFrames, mode: SendPathMode) -> rootcause::Result<SendPathProbeResult> {
    let expected = frames
        .0
        .iter()
        .map(|frame| ExpectedPayload {
            length: frame.as_bytes().len(),
            pointer: frame.as_bytes().as_ptr() as usize,
        })
        .collect::<Vec<_>>();
    let metrics = Arc::new(Mutex::new(ProbeMetrics::new(expected)?));
    let io = ProbeIo {
        metrics: Arc::clone(&metrics),
    };
    let mut state = SendPathState::new(mode, io);
    let result = self::run_frames(&mut state, frames).await?;
    let observed = {
        let metrics = metrics
            .lock()
            .map_err(|_| rootcause::report!("muxr send-path probe metrics lock poisoned"))?;
        if metrics.next_frame != metrics.expected.len() {
            return Err(
                rootcause::report!("muxr send-path probe did not observe every frame").attach(format!(
                    "expected={} actual={}",
                    metrics.expected.len(),
                    metrics.next_frame
                )),
            );
        }
        if metrics.wire_bytes != result.wire_bytes {
            return Err(rootcause::report!("muxr send-path probe transport byte count mismatch")
                .attach(format!("expected={} actual={}", result.wire_bytes, metrics.wire_bytes)));
        }
        SendPathProbeResult {
            frames: result.frames,
            payload_bytes_copied: metrics.payload_bytes.saturating_sub(metrics.direct_payload_bytes),
            payload_copies: result.frames.saturating_sub(metrics.direct_payload_frames),
            wire_bytes: result.wire_bytes,
        }
    };
    Ok(observed)
}

async fn run_frames<T>(state: &mut SendPathState<T>, frames: PreparedSendFrames) -> rootcause::Result<SendPathResult>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let mut result = SendPathResult {
        frames: 0,
        wire_bytes: 0,
    };
    for frame in frames.0 {
        let payload_len = frame.as_bytes().len();
        result.frames = result.frames.saturating_add(1);
        result.wire_bytes = result
            .wire_bytes
            .saturating_add(u64::try_from(size_of::<u32>().saturating_add(payload_len))?);
        state.send(frame.into_bytes()).await?;
    }
    Ok(result)
}

enum SendPathState<T> {
    CopyingCodec(CopyingCodecState<T>),
    Vectored(FrameWriter<T>),
}

impl<T> SendPathState<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn new(mode: SendPathMode, io: T) -> Self {
        match mode {
            SendPathMode::CopyingCodec => {
                // Keep the stream half alive so this reproduces the accepted Framed::split send path, including BiLock.
                let (writer, reader) = Framed::new(io, LengthDelimitedCodec::new()).split();
                Self::CopyingCodec(CopyingCodecState {
                    writer,
                    _reader: reader,
                })
            }
            SendPathMode::Vectored => Self::Vectored(FrameWriter::new(io)),
        }
    }

    async fn send(&mut self, frame: Bytes) -> io::Result<()> {
        match self {
            Self::CopyingCodec(state) => state.writer.send(frame).await,
            Self::Vectored(writer) => writer.send(frame).await,
        }
    }
}

struct CopyingCodecState<T> {
    writer: SplitSink<CopyingFramed<T>, Bytes>,
    _reader: SplitStream<CopyingFramed<T>>,
}

struct DiscardIo;

impl AsyncRead for DiscardIo {
    fn poll_read(self: Pin<&mut Self>, _cx: &mut Context<'_>, _buffer: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for DiscardIo {
    fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buffer: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buffer.len()))
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
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

#[derive(Clone, Copy)]
struct ExpectedPayload {
    length: usize,
    pointer: usize,
}

struct ProbeMetrics {
    direct_payload_bytes: u64,
    direct_payload_frames: u64,
    expected: Vec<ExpectedPayload>,
    next_frame: usize,
    payload_bytes: u64,
    wire_bytes: u64,
}

impl ProbeMetrics {
    fn new(expected: Vec<ExpectedPayload>) -> rootcause::Result<Self> {
        let payload_bytes = expected.iter().try_fold(0_u64, |total, payload| {
            Ok::<_, rootcause::Report>(total.saturating_add(u64::try_from(payload.length)?))
        })?;
        Ok(Self {
            direct_payload_bytes: 0,
            direct_payload_frames: 0,
            expected,
            next_frame: 0,
            payload_bytes,
            wire_bytes: 0,
        })
    }

    fn record_write(&mut self, buffers: &[IoSlice<'_>]) -> io::Result<usize> {
        let Some(expected) = self.expected.get(self.next_frame) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "muxr send-path probe observed an extra write",
            ));
        };
        let written = buffers.iter().try_fold(0_usize, |total, buffer| {
            total
                .checked_add(buffer.len())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "muxr send-path probe write overflow"))
        })?;
        let expected_wire_bytes = size_of::<u32>().saturating_add(expected.length);
        if written != expected_wire_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("muxr send-path probe expected {expected_wire_bytes} wire bytes but observed {written}"),
            ));
        }
        if buffers
            .iter()
            .any(|buffer| buffer.as_ptr() as usize == expected.pointer && buffer.len() == expected.length)
        {
            self.direct_payload_frames = self.direct_payload_frames.saturating_add(1);
            self.direct_payload_bytes = self
                .direct_payload_bytes
                .saturating_add(u64::try_from(expected.length).unwrap_or(u64::MAX));
        }
        self.next_frame = self.next_frame.saturating_add(1);
        self.wire_bytes = self
            .wire_bytes
            .saturating_add(u64::try_from(written).unwrap_or(u64::MAX));
        Ok(written)
    }
}

struct ProbeIo {
    metrics: Arc<Mutex<ProbeMetrics>>,
}

impl AsyncRead for ProbeIo {
    fn poll_read(self: Pin<&mut Self>, _cx: &mut Context<'_>, _buffer: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for ProbeIo {
    fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buffer: &[u8]) -> Poll<io::Result<usize>> {
        let mut metrics = self
            .get_mut()
            .metrics
            .lock()
            .map_err(|_| io::Error::other("muxr send-path probe metrics lock poisoned"))?;
        Poll::Ready(metrics.record_write(&[IoSlice::new(buffer)]))
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let mut metrics = self
            .get_mut()
            .metrics
            .lock()
            .map_err(|_| io::Error::other("muxr send-path probe metrics lock poisoned"))?;
        Poll::Ready(metrics.record_write(buffers))
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

#[cfg(test)]
mod tests {
    use rootcause::prelude::ResultExt;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_probe_send_path_when_mode_varies_observes_payload_copy_difference() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to build muxr transport probe test runtime")?;
        let payload = b"payload";

        let copying = runtime.block_on(self::probe_send_path(
            PreparedSendFrames::new(vec![ProtocolFrame::from(payload.as_slice())]),
            SendPathMode::CopyingCodec,
        ))?;
        let vectored = runtime.block_on(self::probe_send_path(
            PreparedSendFrames::new(vec![ProtocolFrame::from(payload.as_slice())]),
            SendPathMode::Vectored,
        ))?;

        assert_that!(
            copying,
            eq(SendPathProbeResult {
                frames: 1,
                payload_bytes_copied: 8,
                payload_copies: 1,
                wire_bytes: 12,
            })
        );
        assert_that!(
            vectored,
            eq(SendPathProbeResult {
                frames: 1,
                payload_bytes_copied: 0,
                payload_copies: 0,
                wire_bytes: 12,
            })
        );
        Ok(())
    }
}
