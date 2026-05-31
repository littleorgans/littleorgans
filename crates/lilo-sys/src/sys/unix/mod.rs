use crate::signal::SignalOutcome;
use crate::{Error, Result};

std::cfg_select! {
    target_os = "linux" => {
        mod linux;
        pub use linux::ProcessExitWatcher;
        pub(crate) use linux::{peer_cred, start_time_probe_for_pid, watch_process_exit};
    }
    target_os = "macos" => {
        mod macos;
        pub use macos::ProcessExitWatcher;
        pub(crate) use macos::{peer_cred, start_time_probe_for_pid, watch_process_exit};
    }
    _ => {
        pub use super::unsupported::ProcessExitWatcher;
        pub(crate) use super::unsupported::{peer_cred, start_time_probe_for_pid, watch_process_exit};
    }
}

pub(crate) fn current_uid() -> libc::uid_t {
    // SAFETY: getuid has no preconditions and only reads the process identity.
    unsafe { libc::getuid() }
}

pub(crate) fn platform_pid(pid: u32) -> Option<libc::pid_t> {
    libc::pid_t::try_from(pid).ok()
}

pub(crate) fn pid_alive(pid: u32) -> bool {
    let Some(pid) = platform_pid(pid) else {
        return false;
    };

    // SAFETY: signal 0 asks the kernel to validate pid without delivering a
    // signal.
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return true;
    }

    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub(crate) fn send_signal(pid: u32, signal: i32) -> Result<SignalOutcome> {
    let platform_pid = platform_pid(pid).ok_or(Error::InvalidPid { pid })?;

    // SAFETY: kill is called with primitive integer arguments only.
    let result = unsafe { libc::kill(platform_pid, signal) };
    if result == 0 {
        return Ok(SignalOutcome::Delivered);
    }

    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(SignalOutcome::ProcessGone);
    }

    Err(Error::io(
        format!("failed to send signal {signal} to pid {pid}"),
        error,
    ))
}
