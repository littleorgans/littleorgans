#![allow(dead_code)]

use std::future::Future;
use std::io::{self, Read, Write};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::creds::PeerCred;
use crate::process::ProcessStartTime;
use crate::signal::SignalOutcome;
use crate::{Error, Result};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::oneshot;

pub struct ProcessExitWatcher;

#[derive(Debug)]
pub(crate) struct IpcListener;

#[derive(Debug)]
pub(crate) struct IpcStream;

#[derive(Debug)]
pub(crate) struct BlockingIpcStream;

pub(crate) fn pid_alive(_pid: u32) -> bool {
    false
}

pub(crate) fn current_uid() -> u32 {
    panic!("current_uid unsupported on this platform");
}

pub(crate) fn peer_cred(_fd: libc::c_int) -> Result<PeerCred> {
    Err(Error::Unsupported(
        "peer credential extraction unsupported on this platform",
    ))
}

#[allow(clippy::unnecessary_wraps)]
pub(crate) fn start_time_probe_for_pid(_pid: u32) -> Result<ProcessStartTime> {
    Ok(ProcessStartTime::Unsupported)
}

pub(crate) fn watch_process_exit(_pid: u32) -> Result<(ProcessExitWatcher, oneshot::Receiver<()>)> {
    Err(Error::Unsupported(
        "process exit watching is not available on this platform",
    ))
}

pub(crate) fn send_signal(_pid: u32, _signal: i32) -> Result<SignalOutcome> {
    Err(Error::Unsupported(
        "process signal delivery is not available on this platform",
    ))
}

pub(crate) fn on_shutdown() -> io::Result<crate::signal::ShutdownSignal> {
    Err(unsupported_io(
        "shutdown signal watching is not available on this platform",
    ))
}

pub(crate) fn bind_ipc(_path: &Path) -> io::Result<IpcListener> {
    Err(unsupported_io(
        "ipc listener bind is not available on this platform",
    ))
}

pub(crate) fn connect_ipc(_path: &Path) -> impl Future<Output = io::Result<IpcStream>> {
    std::future::ready(Err(unsupported_io(
        "ipc connect is not available on this platform",
    )))
}

pub(crate) fn connect_blocking_ipc(_path: &Path) -> io::Result<BlockingIpcStream> {
    Err(unsupported_io(
        "blocking ipc connect is not available on this platform",
    ))
}

pub(crate) fn remove_socket_file(_path: &Path) -> io::Result<()> {
    Err(unsupported_io(
        "ipc socket cleanup is not available on this platform",
    ))
}

impl IpcListener {
    pub(crate) fn accept(&self) -> impl Future<Output = io::Result<IpcStream>> {
        let _ = self;
        std::future::ready(Err(unsupported_io(
            "ipc accept is not available on this platform",
        )))
    }
}

impl IpcStream {
    pub(crate) fn as_raw_fd(&self) -> libc::c_int {
        let _ = self;
        -1
    }
}

impl AsyncRead for IpcStream {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(unsupported_io(
            "ipc read is not available on this platform",
        )))
    }
}

impl AsyncWrite for IpcStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(unsupported_io(
            "ipc write is not available on this platform",
        )))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(unsupported_io(
            "ipc flush is not available on this platform",
        )))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(unsupported_io(
            "ipc shutdown is not available on this platform",
        )))
    }
}

impl Read for BlockingIpcStream {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(unsupported_io(
            "blocking ipc read is not available on this platform",
        ))
    }
}

impl Write for BlockingIpcStream {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(unsupported_io(
            "blocking ipc write is not available on this platform",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(unsupported_io(
            "blocking ipc flush is not available on this platform",
        ))
    }
}

fn unsupported_io(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}
