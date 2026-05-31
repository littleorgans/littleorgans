use anyhow::{Result, bail};
use lilo_rm_core::{KillOutcome, RuntimeSignal};
use lilo_sys::signal::SignalOutcome;

pub fn send_signal(pid: u32, signal: RuntimeSignal) -> Result<()> {
    send_raw_signal(pid, signal_number(signal))
}

pub fn send_signal_for_kill(pid: u32, signal: RuntimeSignal) -> Result<KillOutcome> {
    send_raw_signal_for_kill(pid, signal_number(signal))
}

pub fn send_raw_signal(pid: u32, signal: i32) -> Result<()> {
    match lilo_sys::signal::send_signal(pid, signal)? {
        SignalOutcome::Delivered => Ok(()),
        SignalOutcome::ProcessGone => {
            bail!("failed to send signal {signal} to pid {pid}: process already exited")
        }
    }
}

pub fn send_raw_signal_for_kill(pid: u32, signal: i32) -> Result<KillOutcome> {
    match lilo_sys::signal::send_signal(pid, signal)? {
        SignalOutcome::Delivered => Ok(KillOutcome::Signalled),
        SignalOutcome::ProcessGone => Ok(KillOutcome::AlreadyExited),
    }
}

pub const fn signal_number(signal: RuntimeSignal) -> i32 {
    match signal {
        RuntimeSignal::Hup => libc::SIGHUP,
        RuntimeSignal::Int => libc::SIGINT,
        RuntimeSignal::Term => libc::SIGTERM,
        RuntimeSignal::Kill => libc::SIGKILL,
    }
}
