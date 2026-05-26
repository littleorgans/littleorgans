//! Host specific process, tmux, signal, and watcher APIs.
//!
//! Platform differences stay isolated here so daemon and shim lifecycle logic
//! remain portable.

#[cfg(target_os = "macos")]
mod kqueue;
pub mod pidfd;
pub mod process;
pub mod process_exit;
pub mod signal;
#[cfg(feature = "test-support")]
pub mod test_support;
pub mod tmux;
