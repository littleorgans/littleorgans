use chrono::{DateTime, Utc};

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStartTime {
    Known(DateTime<Utc>),
    Gone,
    Unsupported,
}

pub fn pid_alive(pid: u32) -> bool {
    crate::sys::pid_alive(pid)
}

pub fn start_time_probe_for_pid(pid: u32) -> Result<ProcessStartTime> {
    crate::sys::start_time_probe_for_pid(pid)
}

pub fn start_time_for_pid(pid: u32) -> Result<Option<DateTime<Utc>>> {
    match start_time_probe_for_pid(pid)? {
        ProcessStartTime::Known(start_time) => Ok(Some(start_time)),
        ProcessStartTime::Gone | ProcessStartTime::Unsupported => Ok(None),
    }
}
