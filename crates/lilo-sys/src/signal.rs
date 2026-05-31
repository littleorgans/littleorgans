use crate::Result;

use std::future::Future;
use std::io;
use std::pin::Pin;

pub(crate) type ShutdownSignal = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalOutcome {
    Delivered,
    ProcessGone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Interrupt,
    Quit,
    Terminate,
}

#[derive(Clone, Copy)]
pub enum SignalDisposition {
    Default,
    Ignore,
    Handler(extern "C" fn(i32)),
}

pub fn send_signal(pid: u32, signal: i32) -> Result<SignalOutcome> {
    crate::sys::send_signal(pid, signal)
}

pub fn install_disposition(signal: Signal, disposition: SignalDisposition) -> io::Result<()> {
    crate::sys::install_signal_disposition(signal, disposition)
}

pub fn on_shutdown() -> io::Result<impl Future<Output = ()> + Send> {
    crate::sys::on_shutdown()
}
