use std::str::FromStr;

use anyhow::{Result, bail};
use lilo_session_core::{
    RpcRequest, RpcResponse, Selector, SmEndpoint, WaitCondition, WaitRequest,
};

use crate::cli::cli_def::WaitArgs;
use crate::cli::output::print_session_table;

pub async fn run(args: WaitArgs) -> Result<()> {
    let endpoint = SmEndpoint::from_env()?;
    let condition = WaitCondition::from_str(&args.condition)?;
    let response = lilo_session_daemon::send_request(
        &endpoint,
        &RpcRequest::Wait {
            request: WaitRequest {
                selector: Selector::from_str(&args.selector)?,
                condition,
                timeout_secs: args.timeout_secs,
            },
        },
    )
    .await?;

    match response {
        RpcResponse::Wait { response } if response.matched => {
            print_session_table(&response.sessions, false);
            Ok(())
        }
        RpcResponse::Wait { .. } => bail!("wait timed out"),
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}
