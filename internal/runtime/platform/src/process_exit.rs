#[cfg(target_os = "macos")]
pub use crate::kqueue::{ProcessExitWatcher, watch_process_exit};

#[cfg(all(not(target_os = "macos"), not(target_os = "linux")))]
use anyhow::Result;
#[cfg(all(not(target_os = "macos"), not(target_os = "linux")))]
use tokio::sync::oneshot;

#[cfg(target_os = "linux")]
mod linux {
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;

    use anyhow::Result;
    use tokio::sync::oneshot;

    const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(100);

    pub struct ProcessExitWatcher {
        cancel: Arc<AtomicBool>,
    }

    impl Drop for ProcessExitWatcher {
        fn drop(&mut self) {
            self.cancel.store(true, Ordering::Release);
        }
    }

    pub fn watch_process_exit(pid: u32) -> Result<(ProcessExitWatcher, oneshot::Receiver<()>)> {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = oneshot::channel();
        let watcher = ProcessExitWatcher {
            cancel: Arc::clone(&cancel),
        };

        match crate::pidfd::open(pid) {
            Ok(pidfd) => spawn_pidfd_wait(pidfd, cancel, sender),
            Err(_) => spawn_liveness_wait(pid, cancel, sender),
        }

        Ok((watcher, receiver))
    }

    fn spawn_pidfd_wait(pidfd: OwnedFd, cancel: Arc<AtomicBool>, sender: oneshot::Sender<()>) {
        std::thread::spawn(move || wait_for_pidfd_exit(pidfd, cancel, sender));
    }

    fn wait_for_pidfd_exit(pidfd: OwnedFd, cancel: Arc<AtomicBool>, sender: oneshot::Sender<()>) {
        while !cancel.load(Ordering::Acquire) {
            let mut poll_fd = libc::pollfd {
                fd: pidfd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: poll_fd points to one initialized pollfd, nfds is 1, and
            // the file descriptor is borrowed from a live OwnedFd for this call.
            let result = unsafe {
                libc::poll(
                    &mut poll_fd,
                    1,
                    WATCH_POLL_INTERVAL.as_millis() as libc::c_int,
                )
            };

            if result > 0 {
                let _ = sender.send(());
                return;
            }
            if result < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                return;
            }
        }
    }

    fn spawn_liveness_wait(pid: u32, cancel: Arc<AtomicBool>, sender: oneshot::Sender<()>) {
        std::thread::spawn(move || {
            while !cancel.load(Ordering::Acquire) {
                if !crate::process::pid_alive(pid) {
                    let _ = sender.send(());
                    return;
                }
                std::thread::sleep(WATCH_POLL_INTERVAL);
            }
        });
    }
}

#[cfg(target_os = "linux")]
pub use linux::{ProcessExitWatcher, watch_process_exit};

#[cfg(all(not(target_os = "macos"), not(target_os = "linux")))]
pub struct ProcessExitWatcher;

#[cfg(all(not(target_os = "macos"), not(target_os = "linux")))]
pub fn watch_process_exit(_pid: u32) -> Result<(ProcessExitWatcher, oneshot::Receiver<()>)> {
    anyhow::bail!("process exit watching is not available on this platform")
}
