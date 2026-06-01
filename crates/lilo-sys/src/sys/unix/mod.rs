use std::io;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, ExitStatus};

use crate::signal::{Signal, SignalDisposition, SignalOutcome};
use crate::{Error, Result};

mod ipc;

pub(crate) use ipc::{
    BlockingIpcStream, IpcListener, IpcStream, bind_ipc, connect_blocking_ipc, connect_ipc,
    remove_socket_file,
};

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

pub(crate) fn reset_child_user_interrupts_before_exec(command: &mut Command) {
    // SAFETY: pre_exec runs in the child after fork and before exec. The
    // closure captures no Rust state and only resets signal dispositions with
    // libc::signal.
    unsafe {
        command.pre_exec(|| {
            set_signal_disposition_raw(libc::SIGINT, libc::SIG_DFL)?;
            set_signal_disposition_raw(libc::SIGQUIT, libc::SIG_DFL)
        });
    }
}

pub(crate) fn exec_replace(command: &mut Command) -> io::Error {
    command.exec()
}

pub(crate) fn exit_signal(status: ExitStatus) -> Option<i32> {
    status.signal()
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

pub(crate) fn install_signal_disposition(
    signal: Signal,
    disposition: SignalDisposition,
) -> io::Result<()> {
    set_signal_disposition_raw(signal_number(signal), disposition_handler(disposition))
}

const fn signal_number(signal: Signal) -> libc::c_int {
    match signal {
        Signal::Interrupt => libc::SIGINT,
        Signal::Quit => libc::SIGQUIT,
        Signal::Terminate => libc::SIGTERM,
    }
}

fn disposition_handler(disposition: SignalDisposition) -> libc::sighandler_t {
    match disposition {
        SignalDisposition::Default => libc::SIG_DFL,
        SignalDisposition::Ignore => libc::SIG_IGN,
        SignalDisposition::Handler(handler) => handler as *const () as libc::sighandler_t,
    }
}

fn set_signal_disposition_raw(
    signal: libc::c_int,
    handler: libc::sighandler_t,
) -> io::Result<()> {
    // SAFETY: signal disposition installation uses primitive signal numbers
    // and handlers only. Callers choose either default, ignore, or an
    // async-signal-safe handler.
    let previous = unsafe { libc::signal(signal, handler) };
    if previous == libc::SIG_ERR {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(crate) fn on_shutdown() -> std::io::Result<crate::signal::ShutdownSignal> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    Ok(Box::pin(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }))
}
