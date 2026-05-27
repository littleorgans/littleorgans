use anyhow::{Result, bail};

use lilo_session_core::{NudgeRequest, RpcResponse, SessionRpc, SmEndpoint};

use crate::cli::cli_def::NudgeArgs;
use crate::cli::selector_scope::required_scoped_selector;

pub async fn run(args: NudgeArgs) -> Result<()> {
    let endpoint = SmEndpoint::from_env()?;
    let response = lilo_session_daemon::send_request(
        &endpoint,
        &SessionRpc::Nudge {
            request: NudgeRequest {
                to: required_scoped_selector(&args.to, &args.scope)?,
                content: args.content,
            },
        },
    )
    .await?;

    match response {
        RpcResponse::Nudged { response } => {
            for nudge in response.nudges {
                println!("{} {}", nudge.to, nudge.message);
            }
            for error in response.errors {
                eprintln!("{} {}", error.target, error.message);
            }
            Ok(())
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}
