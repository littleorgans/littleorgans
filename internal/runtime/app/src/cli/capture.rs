use anyhow::{Result, bail};
use clap::Args;
use lilo_rm_core::{CaptureRequest, RuntimeResponse, RuntimeRpc};
use uuid::Uuid;

use crate::cli::output;

#[derive(Debug, Args)]
pub struct CaptureArgs {
    #[command(flatten)]
    output: output::OutputArgs,
    #[arg(value_name = "SESSION_ID")]
    session_id: Uuid,
    #[arg(long)]
    scrollback_lines: Option<u32>,
}

pub async fn run(args: CaptureArgs) -> Result<()> {
    let socket_path = crate::shared::socket_path()?;
    let response = crate::shared::request(
        &socket_path,
        RuntimeRpc::Capture {
            request: CaptureRequest {
                session_id: args.session_id,
                scrollback_lines: args.scrollback_lines,
            },
        },
    )
    .await?;

    match response {
        RuntimeResponse::Capture(payload) => match payload.response.into_result() {
            Ok(snapshot) => output::emit(&args.output, &snapshot)?,
            Err(error) => bail!(
                "capture failed; error={error:?} session_id={}",
                args.session_id
            ),
        },
        other => bail!("unexpected capture response: {other:?}"),
    }
    Ok(())
}
