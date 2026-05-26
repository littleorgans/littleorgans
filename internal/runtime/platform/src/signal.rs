use anyhow::{Context, Result};
use lilo_rm_core::{KillOutcome, RuntimeSignal};

pub fn send_signal(pid: u32, signal: RuntimeSignal) -> Result<()> {
    send_raw_signal(pid, signal_number(signal))
        .with_context(|| format!("failed to send SIG{signal} to pid {pid}"))
}

pub fn send_signal_for_kill(pid: u32, signal: RuntimeSignal) -> Result<KillOutcome> {
    send_raw_signal_for_kill(pid, signal_number(signal))
        .with_context(|| format!("failed to send SIG{signal} to pid {pid}"))
}

pub fn send_raw_signal(pid: u32, signal: i32) -> Result<()> {
    send_raw_signal_result(pid, signal, false).map(|_| ())
}

pub fn send_raw_signal_for_kill(pid: u32, signal: i32) -> Result<KillOutcome> {
    send_raw_signal_result(pid, signal, true)
}

fn send_raw_signal_result(pid: u32, signal: i32, already_exited_ok: bool) -> Result<KillOutcome> {
    let platform_pid = crate::process::platform_pid(pid)
        .with_context(|| format!("pid {pid} exceeds platform pid range"))?;

    // SAFETY: kill is called with a process id supplied by rtmd state and a
    // signal number supplied by the admin surface.
    let result = unsafe { libc::kill(platform_pid, signal) };
    if result == 0 {
        return Ok(KillOutcome::Signalled);
    }
    let error = std::io::Error::last_os_error();
    if already_exited_ok && error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(KillOutcome::AlreadyExited);
    }
    Err(error).with_context(|| format!("failed to send signal {signal} to pid {pid}"))
}

pub const fn signal_number(signal: RuntimeSignal) -> i32 {
    match signal {
        RuntimeSignal::Hup => libc::SIGHUP,
        RuntimeSignal::Int => libc::SIGINT,
        RuntimeSignal::Term => libc::SIGTERM,
        RuntimeSignal::Kill => libc::SIGKILL,
    }
}
