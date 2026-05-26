#[cfg(target_os = "linux")]
pub fn enabled() -> bool {
    true
}

#[cfg(not(target_os = "linux"))]
pub fn enabled() -> bool {
    false
}

#[cfg(target_os = "linux")]
pub fn open(pid: u32) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;

    // SAFETY: pidfd_open receives integer arguments only and does not
    // dereference Rust pointers.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }

    // SAFETY: fd is non-negative and returned by pidfd_open, so transferring
    // unique ownership into OwnedFd is valid.
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as std::os::fd::RawFd) })
}
