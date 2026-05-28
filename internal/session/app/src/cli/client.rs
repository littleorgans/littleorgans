use anyhow::{Context, Result};
use lilo_paths::{DaemonEndpoint, LiloHome, LiloPaths};
use lilo_session_core::{RpcResponse, SessionRpc};

pub fn paths_from_env() -> Result<LiloPaths> {
    let home = LiloHome::from_env().context("failed to resolve lilo home")?;
    Ok(LiloPaths::new(home))
}

pub fn endpoint_from_env() -> Result<DaemonEndpoint> {
    Ok(DaemonEndpoint::from_paths(&paths_from_env()?))
}

pub async fn send_request(request: &SessionRpc) -> Result<RpcResponse> {
    let endpoint = endpoint_from_env()?;
    lilo_session_daemon::send_request(&endpoint, request).await
}
