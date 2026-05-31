#[cfg(unix)]
use std::os::fd::AsRawFd;

#[cfg(unix)]
use tokio::net::UnixStream;

use crate::{AuthzError, Principal};

#[cfg(unix)]
#[allow(clippy::unused_async)]
pub async fn extract(stream: &UnixStream) -> Result<Principal, AuthzError> {
    let credentials = lilo_sys::creds::peer_cred(stream.as_raw_fd())
        .map_err(|error| internal_error("failed", error))?;

    Ok(principal_from_uid(credentials.uid))
}

fn principal_from_uid(uid: u32) -> Principal {
    Principal::local(uid)
}

fn internal_error(context: &str, error: impl std::fmt::Display) -> AuthzError {
    AuthzError::Internal {
        message: format!("peer credential extraction {context}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::principal_from_uid;
    use crate::Principal;

    #[test]
    fn principal_from_uid_preserves_edge_uids() {
        assert_eq!(principal_from_uid(0), Principal::local(0));
        assert_eq!(principal_from_uid(u32::MAX), Principal::local(u32::MAX));
    }
}
