#[cfg(target_os = "macos")]
use std::os::fd::{AsRawFd, BorrowedFd};

#[cfg(target_os = "linux")]
use nix::sys::socket::{getsockopt, sockopt};
#[cfg(unix)]
use tokio::net::UnixStream;

use crate::{AuthzError, Principal};

#[cfg(target_os = "macos")]
pub async fn extract(stream: &UnixStream) -> Result<Principal, AuthzError> {
    let raw_fd = stream.as_raw_fd();
    let uid = tokio::task::spawn_blocking(move || {
        // Safety: `extract` borrows the stream until this task is awaited, so the fd stays open.
        let fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
        nix::unistd::getpeereid(fd).map(|(uid, _gid)| uid.as_raw())
    })
    .await
    .map_err(|error| internal_error("peer credential task failed", error))?
    .map_err(|error| internal_error("getpeereid failed", error))?;

    Ok(principal_from_uid(uid))
}

#[cfg(target_os = "linux")]
pub async fn extract(stream: &UnixStream) -> Result<Principal, AuthzError> {
    let credentials = getsockopt(stream, sockopt::PeerCredentials)
        .map_err(|error| internal_error("SO_PEERCRED failed", error))?;

    Ok(principal_from_uid(credentials.uid()))
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
pub async fn extract(_stream: &UnixStream) -> Result<Principal, AuthzError> {
    Err(unsupported_platform_error())
}

fn principal_from_uid(uid: u32) -> Principal {
    Principal::Local(uid)
}

fn internal_error(context: &str, error: impl std::fmt::Display) -> AuthzError {
    AuthzError::Internal {
        message: format!("peer credential extraction {context}: {error}"),
    }
}

#[cfg(any(test, all(unix, not(any(target_os = "macos", target_os = "linux")))))]
fn unsupported_platform_error() -> AuthzError {
    AuthzError::Internal {
        message: "peer credential extraction unsupported on this platform".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{principal_from_uid, unsupported_platform_error};
    use crate::{AuthzError, Principal};

    #[test]
    fn principal_from_uid_preserves_edge_uids() {
        assert_eq!(principal_from_uid(0), Principal::Local(0));
        assert_eq!(principal_from_uid(u32::MAX), Principal::Local(u32::MAX));
    }

    #[test]
    fn unsupported_platform_error_is_descriptive() {
        let error = unsupported_platform_error();

        assert!(matches!(
            error,
            AuthzError::Internal { message }
                if message.contains("unsupported on this platform")
        ));
    }
}
