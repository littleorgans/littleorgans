use std::str::FromStr;

use anyhow::{Result, bail};
use lilo_session_core::{LabelMutation, LabelRequest, RpcResponse, SessionRpc, SmEndpoint};

use crate::cli::cli_def::LabelArgs;
use crate::cli::output::print_session_line;
use crate::cli::selector_scope::required_scoped_selector;

pub async fn run(args: LabelArgs) -> Result<()> {
    let endpoint = SmEndpoint::from_env()?;
    let response = lilo_session_daemon::send_request(
        &endpoint,
        &SessionRpc::Label {
            request: LabelRequest {
                selector: required_scoped_selector(&args.selector, &args.scope)?,
                mutation: LabelMutation::from_str(&args.mutation)?,
            },
        },
    )
    .await?;

    match response {
        RpcResponse::Labeled { response } => {
            for session in response.sessions {
                print_session_line(&session, false);
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
