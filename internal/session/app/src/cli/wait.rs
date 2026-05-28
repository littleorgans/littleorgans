use std::str::FromStr;

use anyhow::{Result, bail};
use lilo_session_core::{RpcResponse, Selector, SessionRpc, WaitCondition, WaitRequest};

use crate::cli::cli_def::WaitArgs;
use crate::cli::output::print_session_table;

pub async fn run(args: WaitArgs) -> Result<()> {
    let condition = WaitCondition::from_str(&args.condition)?;
    let response = crate::cli::client::send_request(&SessionRpc::Wait {
        request: WaitRequest {
            selector: Selector::from_str(&args.selector)?,
            condition,
            timeout_secs: args.timeout_secs,
        },
    })
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
