use anyhow::{Result, bail};

use crate::cli::output::{self, OutputArgs};

pub async fn run(output_args: OutputArgs) -> Result<()> {
    let socket_path = crate::shared::socket_path()?;
    let response = crate::shared::request(&socket_path, lilo_rm_core::RuntimeRpc::Version).await?;
    match response {
        lilo_rm_core::RuntimeResponse::Version(payload) => {
            output::emit(&output_args, &payload.version)
        }
        other => bail!("unexpected version response: {other:?}"),
    }
}
