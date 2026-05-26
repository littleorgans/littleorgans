#![cfg(unix)]

use std::path::{Path, PathBuf};

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
use lilo_im_core::AuthzError;
use lilo_im_core::{Principal, peer_creds};
use tokio::net::{UnixListener, UnixStream};

#[tokio::test]
async fn extracts_local_principal_from_accepted_unix_socket() {
    let socket = SocketFile::new();
    let listener = UnixListener::bind(socket.path()).expect("bind peer credential test socket");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client socket");

        peer_creds::extract(&stream)
            .await
            .expect("extract peer credentials")
    });

    let client = UnixStream::connect(socket.path())
        .await
        .expect("connect client socket");
    let principal = server.await.expect("server task should finish");

    assert_eq!(principal, Principal::Local(nix::unistd::getuid().as_raw()));
    drop(client);
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
#[tokio::test]
async fn unsupported_platform_returns_internal_error() {
    let (stream, _peer) = UnixStream::pair().expect("create unix stream pair");
    let error = peer_creds::extract(&stream)
        .await
        .expect_err("unsupported platform should return an error");

    assert!(matches!(
        error,
        AuthzError::Internal { message }
            if message.contains("unsupported on this platform")
    ));
}

struct SocketFile {
    _temp_dir: tempfile::TempDir,
    path: PathBuf,
}

impl SocketFile {
    fn new() -> Self {
        let temp_dir = tempfile::Builder::new()
            .prefix("im-core-peer-creds-")
            .tempdir_in(std::env::temp_dir())
            .expect("create temp dir for socket");
        let path = temp_dir.path().join("peer-creds.sock");

        Self {
            _temp_dir: temp_dir,
            path,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
