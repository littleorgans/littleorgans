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
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) })
}
