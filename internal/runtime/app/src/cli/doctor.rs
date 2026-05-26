use anyhow::{Result, bail};
use lilo_rm_core::{RuntimeResponse, RuntimeRpc};

use crate::cli::output::{self, OutputArgs};

pub async fn run(output_args: OutputArgs) -> Result<()> {
    let socket_path = crate::shared::socket_path()?;
    let response = crate::shared::request(&socket_path, RuntimeRpc::Doctor).await?;
    match response {
        RuntimeResponse::Doctor(payload) => output::emit(&output_args, &payload.doctor),
        other => bail!("unexpected doctor response: {other:?}"),
    }
}
