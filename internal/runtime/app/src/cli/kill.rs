use anyhow::Result;
use clap::Args;
use lilo_rm_client::RuntimeClient;
use lilo_rm_core::{KillByPidRequest, KillRequest, KilledPayload, RuntimeSignal};
use uuid::Uuid;

use crate::cli::output;

#[derive(Debug, Args)]
pub struct KillArgs {
    #[command(flatten)]
    output: output::OutputArgs,
    #[arg(
        value_name = "SESSION_ID",
        conflicts_with = "pid",
        required_unless_present = "pid"
    )]
    session_id: Option<Uuid>,
    #[arg(long, conflicts_with = "session_id")]
    pid: Option<u32>,
    #[arg(long, default_value_t = RuntimeSignal::Term)]
    signal: RuntimeSignal,
    #[arg(long, default_value_t = 2)]
    grace_secs: u64,
}

pub async fn run(args: KillArgs) -> Result<()> {
    if let Some(pid) = args.pid {
        return kill_pid(args, pid).await;
    }
    let session_id = args
        .session_id
        .ok_or_else(|| anyhow::anyhow!("session id or --pid is required"))?;
    let client = RuntimeClient::new(crate::shared::socket_path()?);
    let outcome = client
        .kill(KillRequest {
            session_id,
            signal: args.signal,
            grace_secs: args.grace_secs,
        })
        .await?;
    output::emit(&args.output, &KilledPayload { outcome })?;
    Ok(())
}

async fn kill_pid(args: KillArgs, pid: u32) -> Result<()> {
    let client = RuntimeClient::new(crate::shared::socket_path()?);
    let outcome = client
        .kill_by_pid(KillByPidRequest {
            pid,
            signal: lilo_runtime_platform::signal::signal_number(args.signal),
            grace_secs: args.grace_secs,
        })
        .await?;
    output::emit(&args.output, &KilledPayload { outcome })?;
    Ok(())
}
