use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalOutcome {
    Delivered,
    ProcessGone,
}

pub fn send_signal(pid: u32, signal: i32) -> Result<SignalOutcome> {
    crate::sys::send_signal(pid, signal)
}
