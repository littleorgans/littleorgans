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

pub struct ProcessExitWatcher {
    cancel: Arc<AtomicBool>,
}

impl Drop for ProcessExitWatcher {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

pub(crate) fn peer_cred(fd: libc::c_int) -> Result<PeerCred> {
    let mut uid = libc::uid_t::default();
    let mut gid = libc::gid_t::default();

    // SAFETY: uid and gid are valid output pointers for the duration of the
    // syscall, and `fd` is borrowed from the caller.
    let result = unsafe {
        libc::getpeereid(
            fd,
            std::ptr::addr_of_mut!(uid),
            std::ptr::addr_of_mut!(gid),
        )
    };
    if result != 0 {
        return Err(Error::io(
            "getpeereid failed",
            std::io::Error::last_os_error(),
        ));
    }

    Ok(PeerCred {
        uid,
        gid,
        pid: None,
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
    let Some(platform_pid) = super::platform_pid(pid) else {
        return Ok(ProcessStartTime::Gone);
    };
    let mut info = std::mem::MaybeUninit::<libc::proc_taskallinfo>::uninit();
    let expected_size = libc::c_int::try_from(std::mem::size_of::<libc::proc_taskallinfo>())
        .map_err(|_| Error::invalid_data("proc_taskallinfo size exceeds libc::c_int"))?;
    // SAFETY: proc_pidinfo writes at most `expected_size` bytes into a valid
    // buffer for PROC_PIDTASKALLINFO. The return value tells us whether the
    // struct is full.
    let read = unsafe {
        libc::proc_pidinfo(
            platform_pid,
            libc::PROC_PIDTASKALLINFO,
            0,
            info.as_mut_ptr().cast(),
            expected_size,
        )
    };

    if read != expected_size {
        let error = std::io::Error::last_os_error();
        if (read == 0 && !super::pid_alive(pid)) || error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(ProcessStartTime::Gone);
        }
        return Err(Error::io(
            format!("failed to read start time for pid {pid}"),
            error,
        ));
    }

    // SAFETY: proc_pidinfo reported that the full proc_taskallinfo struct was
    // initialized.
    let info = unsafe { info.assume_init() };
    let seconds = i64::try_from(info.pbsd.pbi_start_tvsec)
        .map_err(|_| Error::invalid_data(format!("invalid start time seconds for pid {pid}")))?;
    let nanos = u32::try_from(info.pbsd.pbi_start_tvusec * 1_000)
        .map_err(|_| Error::invalid_data(format!("invalid start time for pid {pid}")))?;
    let timestamp = Utc
        .timestamp_opt(seconds, nanos)
        .single()
        .ok_or_else(|| Error::invalid_data(format!("invalid start time for pid {pid}")))?;
    Ok(ProcessStartTime::Known(timestamp))
}

pub(crate) fn watch_process_exit(pid: u32) -> Result<(ProcessExitWatcher, oneshot::Receiver<()>)> {
    // SAFETY: kqueue takes no Rust pointers and returns an owned kernel queue
    // descriptor or -1.
    let kq = unsafe { libc::kqueue() };
    if kq < 0 {
        return Err(Error::io(
            "failed to create kqueue",
            std::io::Error::last_os_error(),
        ));
    }

    let change = process_exit_event(pid, libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT);
    // SAFETY: change is a valid kevent, kq is an open queue descriptor, and the
    // call requests no output events by passing a null event list.
    let registered = unsafe {
        libc::kevent(
            kq,
            std::ptr::from_ref(&change),
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    };
    if registered < 0 {
        let error = std::io::Error::last_os_error();
        // SAFETY: kq is still owned by this function on the registration error
        // path and has not been handed to the wait thread.
        unsafe {
            libc::close(kq);
        }
        return Err(Error::io(
            "failed to register kqueue process exit watcher",
            error,
        ));
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = oneshot::channel();
    std::thread::spawn({
        let wait_cancel = Arc::clone(&cancel);
        move || wait_for_exit(kq, pid, wait_cancel.as_ref(), sender)
    });

    Ok((ProcessExitWatcher { cancel }, receiver))
}

fn wait_for_exit(kq: libc::c_int, pid: u32, cancel: &AtomicBool, sender: oneshot::Sender<()>) {
    let mut event = empty_event();
    let timeout = libc::timespec {
        tv_sec: 0,
        tv_nsec: 100_000_000,
    };

    loop {
        if cancel.load(Ordering::Acquire) {
            unregister_and_close(kq, pid);
            return;
        }

        // SAFETY: event is writable storage for one kevent, timeout lives for
        // this call, no changelist is supplied, and the wait thread owns kq.
        let result = unsafe {
            libc::kevent(
                kq,
                std::ptr::null(),
                0,
                std::ptr::from_mut(&mut event),
                1,
                &raw const timeout,
            )
        };
        if result > 0 {
            let _ = sender.send(());
            // SAFETY: this wait thread owns kq after registration and closes it
            // exactly once before returning.
            unsafe {
                libc::close(kq);
            }
            return;
        }
        if result < 0 {
            // SAFETY: this wait thread owns kq after registration and closes it
            // exactly once on the error path.
            unsafe {
                libc::close(kq);
            }
            return;
        }
    }
}

fn unregister_and_close(kq: libc::c_int, pid: u32) {
    let change = process_exit_event(pid, libc::EV_DELETE);
    // SAFETY: change is a valid delete event for pid, no output events are
    // requested, and this function owns kq for the subsequent close.
    unsafe {
        libc::kevent(
            kq,
            std::ptr::from_ref(&change),
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        );
        libc::close(kq);
    }
}

fn process_exit_event(pid: u32, flags: u16) -> libc::kevent {
    libc::kevent {
        ident: pid as libc::uintptr_t,
        filter: libc::EVFILT_PROC,
        flags,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    }
}

fn empty_event() -> libc::kevent {
    process_exit_event(0, 0)
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::start_time_probe_for_pid;
    use crate::process::{ProcessStartTime, start_time_for_pid};

    #[test]
    fn start_time_for_pid_returns_none_for_zombie_process() {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg("exit 0")
            .spawn()
            .expect("spawn short lived process");
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(2);

        while Instant::now() < deadline {
            match start_time_probe_for_pid(pid) {
                Ok(ProcessStartTime::Gone) => {
                    assert_eq!(start_time_for_pid(pid).expect("legacy start time"), None);
                    child.wait().expect("reap child");
                    return;
                }
                Ok(ProcessStartTime::Known(_)) => thread::sleep(Duration::from_millis(10)),
                Ok(ProcessStartTime::Unsupported) => {
                    let _ = child.wait();
                    panic!("macOS start time probe returned unsupported");
                }
                Err(error) => {
                    let _ = child.wait();
                    panic!("start time lookup failed for zombie pid {pid}: {error:#}");
                }
            }
        }

        let _ = child.wait();
        panic!("process {pid} did not reach zombie start time state");
    }
}
