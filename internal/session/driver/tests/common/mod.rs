#[path = "../../../test_support.rs"]
mod shared_test_support;
pub use shared_test_support::OrPanic;

use std::future::Future;

use lilo_session_driver::RtmdDriver;
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

pub fn mock_rtmd_server<F, Fut>(handler: F) -> (RtmdDriver, JoinHandle<()>)
where
    F: FnOnce(UnixStream) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let tempdir = tempfile::tempdir().or_panic("tempdir");
    let socket_path = tempdir.path().join("rtmd.sock");
    let listener = UnixListener::bind(&socket_path).or_panic("bind test socket");
    let driver = RtmdDriver::new(socket_path);
    let server = tokio::spawn(async move {
        let _tempdir = tempdir;
        let (stream, _) = listener.accept().await.or_panic("accept client");
        handler(stream).await;
    });
    (driver, server)
}
