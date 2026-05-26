use std::time::Duration;

use anyhow::Result;
use clap::Subcommand;
use lilo_rm_core::{RuntimeResponse, RuntimeRpc};

#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    Start,
    Stop,
    Status,
    Logs,
}

impl DaemonCommand {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Start => {
                let config = lilo_runtime_daemon::DaemonConfig::from_env()?;
                lilo_runtime_daemon::run_daemon(config).await
            }
            Self::Stop => stop().await,
            Self::Status => status(),
            Self::Logs => {
                println!("daemon logs are not persisted in pass 1");
                Ok(())
            }
        }
    }
}

async fn stop() -> Result<()> {
    let socket_path = crate::shared::socket_path()?;
    match crate::shared::request(&socket_path, RuntimeRpc::Stop).await? {
        RuntimeResponse::Stopping => {
            crate::shared::wait_for_socket_removed(&socket_path, Duration::from_secs(2)).await?;
            println!("rtmd stopped");
            Ok(())
        }
        other => anyhow::bail!("unexpected stop response: {other:?}"),
    }
}

fn status() -> Result<()> {
    let socket_path = crate::shared::socket_path()?;
    if socket_path.exists() {
        println!("rtmd socket present at {}", socket_path.display());
    } else {
        println!("rtmd not running");
    }
    Ok(())
}
