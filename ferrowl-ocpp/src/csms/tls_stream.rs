//! A uniform accepted-socket type so the accept loop can hand `accept_hdr_async` a single
//! concrete stream regardless of whether Security Profile 2/3 TLS is configured.

use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

/// A server-side socket that is either plain TCP or TLS-terminated TCP.
pub(crate) enum ServerStream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::server::TlsStream<TcpStream>>),
}

impl AsyncRead for ServerStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ServerStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            ServerStream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ServerStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            ServerStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            ServerStream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ServerStream::Plain(s) => Pin::new(s).poll_flush(cx),
            ServerStream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ServerStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            ServerStream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}
