use anyhow::{Result, bail};
use clap::Args;
use lilo_rm_core::{NudgeFailureReason, NudgeOutcome, NudgeRequest, RuntimeResponse, RuntimeRpc};
use uuid::Uuid;

use crate::cli::output;

#[derive(Debug, Args)]
pub struct NudgeArgs {
    #[command(flatten)]
    output: output::OutputArgs,
    #[arg(value_name = "SESSION_ID")]
    session_id: Uuid,
    #[arg(long)]
    content: String,
}

pub async fn run(args: NudgeArgs) -> Result<()> {
    let socket_path = crate::shared::socket_path()?;
    let response = crate::shared::request(
        &socket_path,
        RuntimeRpc::Nudge {
            request: NudgeRequest {
                session_id: args.session_id,
                content: args.content,
            },
        },
    )
    .await?;

    match response {
        RuntimeResponse::Nudge(payload) if payload.response.delivered => {
            output::emit(&args.output, &payload.response)?;
        }
        RuntimeResponse::Nudge(payload) => match payload.response.outcome {
            NudgeOutcome::Unsupported(reason) => bail!(
                "nudge unsupported; reason={} session_id={}",
                reason.as_str(),
                args.session_id
            ),
            NudgeOutcome::Failed(reason) => {
                bail!("{}", nudge_failed_message(reason, args.session_id))
            }
            NudgeOutcome::Delivered => bail!("inconsistent nudge response: {:?}", payload.response),
        },
        other => bail!("unexpected nudge response: {other:?}"),
    }
    Ok(())
}

fn nudge_failed_message(reason: NudgeFailureReason, session_id: Uuid) -> String {
    match reason {
        NudgeFailureReason::SessionEnded => format!(
            "nudge failed; reason={} session_id={} detail=session is no longer running",
            reason.as_str(),
            session_id
        ),
        _ => format!(
            "nudge failed; reason={} session_id={}",
            reason.as_str(),
            session_id
        ),
    }
}
