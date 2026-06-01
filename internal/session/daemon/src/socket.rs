use std::time::Duration;

use anyhow::{Context, Result};
use lilo_paths::DaemonEndpoint;
use lilo_rm_core::{read_json_line, write_json_line};
use lilo_session_core::{RpcResponse, SessionRpc};
use lilo_wire::LilodRpc;
use tokio::io::{AsyncRead, BufReader};
use tokio::time;

pub async fn send_request(endpoint: &DaemonEndpoint, request: &SessionRpc) -> Result<RpcResponse> {
    let stream = lilo_sys::ipc::connect(endpoint.as_path())
        .await
        .with_context(|| format!("failed to connect to {endpoint}"))?;
    let (read_half, mut write_half) = stream.into_split();
    write_json_line(&mut write_half, &LilodRpc::Session(request.clone()))
        .await
        .context("failed to write request")?;

    read_response(read_half).await
}

pub async fn send_request_with_timeout(
    endpoint: &DaemonEndpoint,
    request: &SessionRpc,
    timeout: Duration,
) -> Result<RpcResponse> {
    time::timeout(timeout, send_request(endpoint, request))
        .await
        .with_context(|| format!("timed out waiting for daemon response after {timeout:?}"))?
}

pub(crate) async fn read_response<R>(read_half: R) -> Result<RpcResponse>
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(read_half);
    read_json_line(&mut reader)
        .await
        .context("failed to decode response")
}
