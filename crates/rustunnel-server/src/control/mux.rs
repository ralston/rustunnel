//! WebSocket ↔ yamux bridge.
//!
//! yamux 0.13 requires `futures::io::{AsyncRead, AsyncWrite}`.
//! WebSocket frames are message-oriented, so we adapt by reading binary
//! message payloads as a contiguous byte stream and writing yamux output back
//! as binary WebSocket messages.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::future::poll_fn;
use futures_util::io::{AsyncRead, AsyncWrite};
use futures_util::sink::Sink;
use futures_util::stream::Stream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use tokio_util::compat::TokioAsyncReadCompatExt;
use uuid::Uuid;
use yamux::{Connection, Mode, Stream as YamuxStream};

// DuplexStream is kept alive for the lifetime of the MuxSession by storing
// the client end inside it; see start_detached.
use tokio::io::DuplexStream;
use tokio_util::compat::Compat;

use crate::error::{Error, Result};

// ── WsCompat ─────────────────────────────────────────────────────────────────

/// Adapts a `WebSocketStream` into `futures::io::{AsyncRead, AsyncWrite}` so
/// that yamux (and the data-plane bridge) can operate on it.
///
/// Read path: binary WebSocket frames are treated as an ordered byte stream.
/// Write path: bytes are wrapped in binary WebSocket frames.
pub struct WsCompat<S> {
    inner: WebSocketStream<S>,
    read_buf: Vec<u8>,
    read_pos: usize,
}

impl<S> WsCompat<S> {
    pub fn new(ws: WebSocketStream<S>) -> Self {
        Self {
            inner: ws,
            read_buf: Vec::new(),
            read_pos: 0,
        }
    }
}

impl<S> AsyncRead for WsCompat<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        loop {
            if this.read_pos < this.read_buf.len() {
                let remaining = &this.read_buf[this.read_pos..];
                let n = remaining.len().min(buf.len());
                buf[..n].copy_from_slice(&remaining[..n]);
                this.read_pos += n;
                return Poll::Ready(Ok(n));
            }

            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(Ok(0)), // EOF
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, e)))
                }
                Poll::Ready(Some(Ok(msg))) => match msg {
                    Message::Binary(data) => {
                        let n = data.len().min(buf.len());
                        buf[..n].copy_from_slice(&data[..n]);
                        if n < data.len() {
                            this.read_buf = data[n..].to_vec();
                            this.read_pos = 0;
                        }
                        return Poll::Ready(Ok(n));
                    }
                    Message::Close(_) => return Poll::Ready(Ok(0)),
                    _ => continue, // skip control frames
                },
            }
        }
    }
}

impl<S> AsyncWrite for WsCompat<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let msg = Message::Binary(buf.to_vec());

        match Pin::new(&mut this.inner).poll_ready(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => {
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, e)))
            }
            Poll::Ready(Ok(())) => {}
        }
        if let Err(e) = Pin::new(&mut this.inner).start_send(msg) {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, e)));
        }
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner)
            .poll_flush(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner)
            .poll_close(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e))
    }
}

// ── MuxSession ───────────────────────────────────────────────────────────────

/// A running yamux session.
///
/// Backed by an in-process loopback pair.  The `pipe_client` end is handed
/// off to the data-plane bridge task once the client's `/_data/<session_id>`
/// WebSocket arrives; that task copies bytes between the real data WebSocket
/// and `pipe_client`, feeding yamux frames into the server-side `Connection`.
pub struct MuxSession {
    pub(crate) conn: Connection<Compat<DuplexStream>>,
    /// Client end of the loopback.  Taken by the data-plane bridge once ready.
    pipe_client: Option<DuplexStream>,
}

impl MuxSession {
    /// Create a `MuxSession` backed by an in-process loopback pair.
    ///
    /// Call [`take_pipe_client`] to retrieve the peer end so it can be
    /// bridged to the real data WebSocket.
    pub fn start_detached() -> Self {
        let (server_side, client_side) = tokio::io::duplex(64 * 1024);
        let conn = Connection::new(server_side.compat(), yamux::Config::default(), Mode::Server);
        Self {
            conn,
            pipe_client: Some(client_side),
        }
    }

    /// Take the loopback peer end.  Returns `None` if already taken.
    pub fn take_pipe_client(&mut self) -> Option<DuplexStream> {
        self.pipe_client.take()
    }

    /// Poll for the next inbound stream from the remote peer.
    pub async fn next_inbound(&mut self) -> Option<yamux::Result<YamuxStream>> {
        poll_fn(|cx| self.conn.poll_next_inbound(cx)).await
    }

    /// Open a new outbound stream to the remote peer.
    pub async fn open_stream(&mut self) -> yamux::Result<YamuxStream> {
        poll_fn(|cx| self.conn.poll_new_outbound(cx)).await
    }
}

// ── open_data_stream ──────────────────────────────────────────────────────────

/// Open a yamux data stream for `conn_id` and return it.
///
/// The caller writes a `DataStreamOpen` control frame so the client can
/// correlate the stream with the pending proxy connection.
pub async fn open_data_stream(mux: &mut MuxSession, conn_id: Uuid) -> Result<YamuxStream> {
    tracing::debug!(%conn_id, "opening yamux data stream");
    mux.open_stream()
        .await
        .map_err(|e| Error::Mux(e.to_string()))
}
