use std::{
    io,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;
use pin_project_lite::pin_project;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_listener::{Listener, ListenerAddress};
use tonic::transport::server::{Connected, TcpConnectInfo, UdsConnectInfo};

/// A wrapper around a [Listener] which implements the [Stream] trait.
/// Mainly used to bridge [tokio_listener] with [tonic].
pub struct ListenerStream {
    inner: Listener,
}

impl ListenerStream {
    /// Convert a [Listener] into a [Stream].
    pub fn new(inner: Listener) -> Self {
        Self { inner }
    }

    /// Binds to the specified address and returns a [Stream] of connections.
    pub async fn bind(addr: &ListenerAddress) -> io::Result<Self> {
        let listener = Listener::bind(addr, &Default::default(), &Default::default()).await?;

        Ok(Self::new(listener))
    }
}

impl Stream for ListenerStream {
    type Item = io::Result<Connection>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.poll_accept(cx) {
            Poll::Ready(Ok((connection, _))) => Poll::Ready(Some(Ok(Connection::new(connection)))),
            Poll::Ready(Err(err)) => Poll::Ready(Some(Err(err))),
            Poll::Pending => Poll::Pending,
        }
    }
}

pin_project! {
    /// A wrapper around a [tokio_listener::Connection] that implements the [Connected] trait
    /// so it is compatible with [tonic].
    pub struct Connection {
        #[pin]
        inner: tokio_listener::Connection,
    }
}

impl Connection {
    fn new(inner: tokio_listener::Connection) -> Self {
        Self { inner }
    }
}

impl Deref for Connection {
    type Target = tokio_listener::Connection;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Connection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[derive(Clone)]
pub enum ListenerConnectInfo {
    TCP(TcpConnectInfo),
    Unix(UdsConnectInfo),
    Stdio,
    Other,
}

impl Connected for Connection {
    type ConnectInfo = ListenerConnectInfo;

    fn connect_info(&self) -> Self::ConnectInfo {
        if let Some(tcp_stream) = self.try_borrow_tcp() {
            ListenerConnectInfo::TCP(tcp_stream.connect_info())
        } else if let Some(unix_stream) = self.try_borrow_unix() {
            ListenerConnectInfo::Unix(unix_stream.connect_info())
        } else if self.try_borrow_stdio().is_some() {
            ListenerConnectInfo::Stdio
        } else {
            ListenerConnectInfo::Other
        }
    }
}

impl AsyncRead for Connection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.project().inner.poll_read(cx, buf)
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, io::Error>> {
        self.project().inner.poll_write(cx, buf)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), io::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), io::Error>> {
        self.project().inner.poll_shutdown(cx)
    }
}
