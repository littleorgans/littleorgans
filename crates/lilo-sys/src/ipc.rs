use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Debug)]
pub struct IpcListener(crate::sys::IpcListener);

#[derive(Debug)]
pub struct IpcStream(crate::sys::IpcStream);

#[derive(Debug)]
pub struct BlockingIpcStream(crate::sys::BlockingIpcStream);

pub type OwnedReadHalf = tokio::io::ReadHalf<IpcStream>;
pub type OwnedWriteHalf = tokio::io::WriteHalf<IpcStream>;

pub fn bind(path: impl AsRef<Path>) -> io::Result<IpcListener> {
    crate::sys::bind_ipc(path.as_ref()).map(IpcListener)
}

pub async fn connect(path: impl AsRef<Path>) -> io::Result<IpcStream> {
    crate::sys::connect_ipc(path.as_ref()).await.map(IpcStream)
}

pub fn connect_blocking(path: impl AsRef<Path>) -> io::Result<BlockingIpcStream> {
    crate::sys::connect_blocking_ipc(path.as_ref()).map(BlockingIpcStream)
}

pub fn remove_socket_file(path: impl AsRef<Path>) -> io::Result<()> {
    crate::sys::remove_socket_file(path.as_ref())
}

impl IpcListener {
    pub async fn accept(&self) -> io::Result<IpcStream> {
        self.0.accept().await.map(IpcStream)
    }
}

impl IpcStream {
    #[must_use]
    pub fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }

    #[must_use]
    pub fn into_split(self) -> (OwnedReadHalf, OwnedWriteHalf) {
        tokio::io::split(self)
    }
}

impl AsRawFd for IpcStream {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl AsyncRead for IpcStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for IpcStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

impl Read for BlockingIpcStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for BlockingIpcStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
