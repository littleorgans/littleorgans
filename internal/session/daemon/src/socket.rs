use anyhow::{Context, Result};
use lilo_rm_core::{read_json_line, write_json_line};
use lilo_session_core::{RpcResponse, SessionRpc, SmEndpoint};
use lilo_wire::LilodRpc;
use tokio::io::BufReader;
use tokio::net::{UnixStream, unix::OwnedReadHalf};

pub async fn send_request(endpoint: &SmEndpoint, request: &SessionRpc) -> Result<RpcResponse> {
    let stream = UnixStream::connect(endpoint.as_path())
        .await
        .with_context(|| format!("failed to connect to {endpoint}"))?;
    let (read_half, mut write_half) = stream.into_split();
    write_json_line(&mut write_half, &LilodRpc::Session(request.clone()))
        .await
        .context("failed to write request")?;

    read_response(read_half).await
}

pub(crate) async fn read_response(read_half: OwnedReadHalf) -> Result<RpcResponse> {
    let mut reader = BufReader::new(read_half);
    read_json_line(&mut reader)
        .await
        .context("failed to decode response")
}
