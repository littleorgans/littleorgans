use anyhow::{Result, bail};
use lilo_rm_core::{CliOutput, RuntimeResponse, RuntimeRpc};

use crate::cli::output::{self, OutputArgs};

pub async fn run(output_args: OutputArgs) -> Result<()> {
    emit_rpc_response(
        &output_args,
        RuntimeRpc::Version,
        |response| match response {
            RuntimeResponse::Version(payload) => Ok(payload.version),
            other => bail!("unexpected version response: {other:?}"),
        },
    )
    .await
}

pub(crate) async fn emit_rpc_response<T>(
    output_args: &OutputArgs,
    rpc: RuntimeRpc,
    extract: impl FnOnce(RuntimeResponse) -> Result<T>,
) -> Result<()>
where
    T: CliOutput,
{
    let socket_path = crate::shared::socket_path()?;
    let response = crate::shared::request(&socket_path, rpc).await?;
    output::emit(output_args, &extract(response)?)
}
