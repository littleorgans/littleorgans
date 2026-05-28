use std::path::PathBuf;

use lilo_rm_client::RuntimeClient;
use lilo_rm_core::{RuntimeResponse, RuntimeRpc, read_json_line, write_json_line};
use lilo_wire::LilodRpc;
use tokio::io::BufReader;
use tokio::net::UnixListener;
use tokio::task::JoinHandle;

pub fn temp_socket_path() -> (tempfile::TempDir, PathBuf) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let socket_path = tempdir.path().join("rtmd.sock");
    (tempdir, socket_path)
}

pub fn mock_runtime_response(
    expected_rpc: RuntimeRpc,
    response: RuntimeResponse,
) -> (RuntimeClient, JoinHandle<()>) {
    mock_runtime_exchange(move |rpc| {
        assert_eq!(rpc, expected_rpc);
        (response, ())
    })
}

pub fn mock_runtime_exchange<T>(
    handler: impl FnOnce(RuntimeRpc) -> (RuntimeResponse, T) + Send + 'static,
) -> (RuntimeClient, JoinHandle<T>)
where
    T: Send + 'static,
{
    let (tempdir, socket_path) = temp_socket_path();
    let listener = UnixListener::bind(&socket_path).expect("bind test socket");
    let client = RuntimeClient::new(socket_path);
    let server = tokio::spawn(async move {
        let _tempdir = tempdir;
        let (stream, _) = listener.accept().await.expect("accept client");
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let envelope: LilodRpc = read_json_line(&mut reader).await.expect("read rpc");
        let LilodRpc::Runtime(rpc) = envelope else {
            panic!("expected runtime rpc");
        };
        let (response, output) = handler(rpc);
        write_json_line(&mut write_half, &response)
            .await
            .expect("write response");
        output
    });
    (client, server)
}
