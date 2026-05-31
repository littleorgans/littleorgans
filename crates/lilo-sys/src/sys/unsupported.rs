#![allow(dead_code)]

use tokio::sync::oneshot;

use crate::process::ProcessStartTime;
use crate::signal::SignalOutcome;
use crate::{Error, Result};

pub struct ProcessExitWatcher;

pub(crate) fn pid_alive(_pid: u32) -> bool {
    false
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
