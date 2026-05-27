use anyhow::{Context, Result};
use lilo_session_core::{RpcRequest, RpcResponse, SmEndpoint};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

pub async fn send_request(endpoint: &SmEndpoint, request: &RpcRequest) -> Result<RpcResponse> {
    let mut stream = UnixStream::connect(endpoint.as_path())
        .await
        .with_context(|| format!("failed to connect to {endpoint}"))?;
    let request = serde_json::to_vec(request).context("failed to encode request")?;
    stream
        .write_all(&request)
        .await
        .context("failed to write request")?;
    stream.shutdown().await.context("failed to close request")?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .context("failed to read response")?;
    serde_json::from_slice(&response).context("failed to decode response")
}
