use tokio::sync::oneshot;

use crate::Result;

pub use crate::sys::ProcessExitWatcher;

pub fn watch_process_exit(pid: u32) -> Result<(ProcessExitWatcher, oneshot::Receiver<()>)> {
    crate::sys::watch_process_exit(pid)
}
