use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use chrono::{TimeZone, Utc};
use tokio::sync::oneshot;

use crate::creds::PeerCred;
use crate::process::ProcessStartTime;
use crate::{Error, Result};

const START_TIME_READ_ATTEMPTS: usize = 5;
const START_TIME_RETRY_DELAY: Duration = Duration::from_millis(10);
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub struct ProcessExitWatcher {
    cancel: Arc<AtomicBool>,
}

impl Drop for ProcessExitWatcher {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

pub(crate) fn peer_cred(fd: libc::c_int) -> Result<PeerCred> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length =
        libc::socklen_t::try_from(std::mem::size_of::<libc::ucred>()).expect("ucred size fits");

    // SAFETY: `credentials` and `length` point to initialized writable memory
    // for the duration of the syscall, and `fd` is borrowed from the caller.
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::addr_of_mut!(credentials).cast::<libc::c_void>(),
            &raw mut length,
        )
    };
    if result != 0 {
        return Err(Error::io(
            "SO_PEERCRED failed",
            std::io::Error::last_os_error(),
        ));
    }

    Ok(PeerCred {
        uid: credentials.uid,
        gid: credentials.gid,
        pid: u32::try_from(credentials.pid).ok(),
    })
}

pub(crate) fn start_time_probe_for_pid(pid: u32) -> Result<ProcessStartTime> {
    for attempt in 1..=START_TIME_READ_ATTEMPTS {
        let probe = read_start_time_probe_for_pid(pid)?;
        if probe != ProcessStartTime::Gone
            || !super::pid_alive(pid)
            || attempt == START_TIME_READ_ATTEMPTS
        {
            return Ok(probe);
        }
        std::thread::sleep(START_TIME_RETRY_DELAY);
    }

    Ok(ProcessStartTime::Gone)
}

fn read_start_time_probe_for_pid(pid: u32) -> Result<ProcessStartTime> {
    let stat_path = format!("/proc/{pid}/stat");
    let stat = match std::fs::read_to_string(&stat_path) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProcessStartTime::Gone);
        }
        Err(error) => return Err(Error::io(format!("failed to read {stat_path}"), error)),
    };

    let Some(start_ticks) = proc_stat_start_ticks(&stat) else {
        return Err(Error::invalid_data(format!(
            "failed to parse start time from {stat_path}"
        )));
    };

    let Some(boot_time) = linux_boot_time(Path::new("/proc/stat"))? else {
        return Ok(ProcessStartTime::Unsupported);
    };
    let Some(start_time) = start_time_from_ticks(boot_time, start_ticks)? else {
        return Ok(ProcessStartTime::Unsupported);
    };

    Ok(ProcessStartTime::Known(start_time))
}

fn proc_stat_start_ticks(stat: &str) -> Option<u64> {
    let close = stat.rfind(')')?;
    let mut fields = stat.get(close + 2..)?.split_whitespace();
    fields.nth(19)?.parse().ok()
}

fn linux_boot_time(path: &Path) -> Result<Option<i64>> {
    let stat = match std::fs::read_to_string(path) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::io(format!("failed to read {}", path.display()), error)),
    };

    Ok(stat.lines().find_map(|line| {
        line.strip_prefix("btime ")
            .and_then(|value| value.trim().parse().ok())
    }))
}

fn start_time_from_ticks(boot_time: i64, start_ticks: u64) -> Result<Option<chrono::DateTime<Utc>>> {
    // SAFETY: sysconf with _SC_CLK_TCK reads a process global setting and does
    // not dereference Rust pointers.
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks_per_second <= 0 {
        return Ok(None);
    }

    let ticks_per_second = u64::try_from(ticks_per_second)
        .map_err(|_| Error::invalid_data("invalid Linux clock tick rate"))?;
    let seconds_since_boot = i64::try_from(start_ticks / ticks_per_second)
        .map_err(|_| Error::invalid_data("process start time overflow"))?;
    let Some(seconds) = boot_time.checked_add(seconds_since_boot) else {
        return Ok(None);
    };
    let nanos =
        (u128::from(start_ticks % ticks_per_second) * 1_000_000_000) / u128::from(ticks_per_second);
    let nanos = u32::try_from(nanos)
        .map_err(|_| Error::invalid_data("process start time nanoseconds overflow"))?;
    Utc.timestamp_opt(seconds, nanos)
        .single()
        .map(Some)
        .ok_or_else(|| Error::invalid_data("invalid Linux process start time"))
}

// Returns Result for signature symmetry with the macOS (kqueue can fail) and
// unsupported (always Err) impls that sys/unix/mod.rs re-exports uniformly; the
// Linux pidfd path falls back to liveness polling and never errors itself.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn watch_process_exit(pid: u32) -> Result<(ProcessExitWatcher, oneshot::Receiver<()>)> {
    let cancel = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = oneshot::channel();
    let watcher = ProcessExitWatcher {
        cancel: Arc::clone(&cancel),
    };

    match open_pidfd(pid) {
        Ok(pidfd) => spawn_pidfd_wait(pidfd, cancel, sender),
        Err(_) => spawn_liveness_wait(pid, cancel, sender),
    }

    Ok((watcher, receiver))
}

fn open_pidfd(pid: u32) -> std::io::Result<OwnedFd> {
    let pid = libc::pid_t::try_from(pid).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "pid out of range for pid_t",
        )
    })?;

    // SAFETY: pidfd_open receives integer arguments only and does not
    // dereference Rust pointers.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let fd = std::os::fd::RawFd::try_from(fd)
        .expect("pidfd_open returns a non-negative value within RawFd range");
    // SAFETY: fd is non-negative and returned by pidfd_open, so transferring
    // unique ownership into OwnedFd is valid.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn spawn_pidfd_wait(pidfd: OwnedFd, cancel: Arc<AtomicBool>, sender: oneshot::Sender<()>) {
    std::thread::spawn(move || wait_for_pidfd_exit(pidfd, cancel, sender));
}

// pidfd is owned so the fd stays open across poll iterations and is closed on
// Drop when the thread exits. cancel is moved in so the Arc handle lives for the
// thread lifetime. needless_pass_by_value misreads this pattern.
#[allow(clippy::needless_pass_by_value)]
fn wait_for_pidfd_exit(pidfd: OwnedFd, cancel: Arc<AtomicBool>, sender: oneshot::Sender<()>) {
    let poll_timeout =
        libc::c_int::try_from(WATCH_POLL_INTERVAL.as_millis()).expect("poll interval fits in c_int");
    while !cancel.load(Ordering::Acquire) {
        let mut poll_fd = libc::pollfd {
            fd: pidfd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll_fd points to one initialized pollfd, nfds is 1, and the
        // file descriptor is borrowed from a live OwnedFd for this call.
        let result = unsafe { libc::poll(&raw mut poll_fd, 1, poll_timeout) };

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
            if !super::pid_alive(pid) {
                let _ = sender.send(());
                return;
            }
            std::thread::sleep(WATCH_POLL_INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::{proc_stat_start_ticks, start_time_probe_for_pid};
    use crate::process::ProcessStartTime;

    #[test]
    fn proc_stat_start_ticks_handles_process_names_with_spaces() {
        let stat = "123 (name with spaces) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 4242 20";

        assert_eq!(proc_stat_start_ticks(stat), Some(4242));
    }

    #[test]
    fn start_time_probe_reads_linux_proc_start_time() {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg("sleep 1")
            .spawn()
            .expect("spawn process");
        let pid = child.id();

        match start_time_probe_for_pid(pid).expect("start time probe") {
            ProcessStartTime::Known(_) => {}
            ProcessStartTime::Gone => panic!("running child was reported gone"),
            ProcessStartTime::Unsupported => panic!("Linux /proc start time was unsupported"),
        }

        child.kill().expect("kill child");
        child.wait().expect("reap child");
        assert_eq!(
            start_time_probe_for_pid(pid).expect("start time after reap"),
            ProcessStartTime::Gone
        );
    }
}
