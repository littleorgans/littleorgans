use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream as BlockingUnixStream;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{UnixListener, UnixStream};

#[derive(Debug)]
pub(crate) struct IpcListener {
    inner: UnixListener,
}

#[derive(Debug)]
pub(crate) struct IpcStream {
    inner: UnixStream,
}

#[derive(Debug)]
pub(crate) struct BlockingIpcStream {
    inner: BlockingUnixStream,
}

pub(crate) fn bind_ipc(path: &Path) -> io::Result<IpcListener> {
    prepare_socket(path)?;
    UnixListener::bind(path).map(|inner| IpcListener { inner })
}

pub(crate) async fn connect_ipc(path: &Path) -> io::Result<IpcStream> {
    UnixStream::connect(path).await.map(|inner| IpcStream { inner })
}

pub(crate) fn connect_blocking_ipc(path: &Path) -> io::Result<BlockingIpcStream> {
    BlockingUnixStream::connect(path).map(|inner| BlockingIpcStream { inner })
}

pub(crate) fn remove_socket_file(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

impl IpcListener {
    pub(crate) async fn accept(&self) -> io::Result<IpcStream> {
        let (inner, _) = self.inner.accept().await?;
        Ok(IpcStream { inner })
    }
}

impl IpcStream {
    pub(crate) fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl AsyncRead for IpcStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for IpcStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl Read for BlockingIpcStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for BlockingIpcStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn prepare_socket(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("socket path {} has no parent", path.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;
    remove_socket_file(path)
}
