#[cfg(target_os = "macos")]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Result;
use tokio::sync::oneshot;

#[cfg(target_os = "macos")]
pub struct ProcessExitWatcher {
    cancel: Arc<AtomicBool>,
}

#[cfg(not(target_os = "macos"))]
pub struct ProcessExitWatcher;

#[cfg(target_os = "macos")]
impl Drop for ProcessExitWatcher {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

#[cfg(target_os = "macos")]
pub fn watch_process_exit(pid: u32) -> Result<(ProcessExitWatcher, oneshot::Receiver<()>)> {
    // SAFETY: kqueue takes no Rust pointers and returns an owned kernel queue
    // descriptor or -1.
    let kq = unsafe { libc::kqueue() };
    if kq < 0 {
        return Err(Into::into(std::io::Error::last_os_error()));
    }

    let change = process_exit_event(pid, libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT);
    // SAFETY: change is a valid kevent, kq is an open queue descriptor, and
    // the call requests no output events by passing a null event list.
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
        return Err(Into::into(error));
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = oneshot::channel();
    std::thread::spawn({
        let wait_cancel = Arc::clone(&cancel);
        move || wait_for_exit(kq, pid, wait_cancel.as_ref(), sender)
    });

    Ok((ProcessExitWatcher { cancel }, receiver))
}

#[cfg(not(target_os = "macos"))]
pub fn watch_process_exit(_pid: u32) -> Result<(ProcessExitWatcher, oneshot::Receiver<()>)> {
    anyhow::bail!("kqueue process exit watching is only available on macOS")
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn empty_event() -> libc::kevent {
    process_exit_event(0, 0)
}
