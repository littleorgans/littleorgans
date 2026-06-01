use chrono::{DateTime, Utc};
use std::io;
use std::process::{Command, ExitStatus};

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

pub fn reset_child_user_interrupts_before_exec(command: &mut Command) {
    crate::sys::reset_child_user_interrupts_before_exec(command);
}

pub fn exec_replace(command: &mut Command) -> io::Error {
    crate::sys::exec_replace(command)
}

pub fn exit_signal(status: ExitStatus) -> Option<i32> {
    crate::sys::exit_signal(status)
}

#[cfg(all(test, target_family = "unix"))]
mod tests {
    use super::exit_signal;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    #[test]
    fn exit_signal_reports_unix_signal() {
        let status = ExitStatus::from_raw(libc::SIGTERM);

        assert_eq!(exit_signal(status), Some(libc::SIGTERM));
    }
}
