use anyhow::{Result, bail};
use lilo_rm_core::{RuntimeResponse, RuntimeRpc};

use crate::cli::output::OutputArgs;

pub async fn run(output_args: OutputArgs) -> Result<()> {
    super::version::emit_rpc_response(
        &output_args,
        RuntimeRpc::Doctor,
        |response| match response {
            RuntimeResponse::Doctor(payload) => Ok(payload.doctor),
            other => bail!("unexpected doctor response: {other:?}"),
        },
    )
    .await
}
