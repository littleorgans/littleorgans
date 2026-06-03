use anyhow::{Result, bail};
use lilo_session_core::{ListRequest, RpcResponse, SessionRpc};

use crate::cli::output::ShortSessionIdSet;

pub async fn load() -> Result<ShortSessionIdSet> {
    let response = crate::cli::client::send_request(&SessionRpc::List {
        request: ListRequest { selector: None },
    })
    .await?;

    match response {
        RpcResponse::Listed { response } => {
            Ok(ShortSessionIdSet::from_sessions(&response.sessions))
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}
