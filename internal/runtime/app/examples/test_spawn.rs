#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "support/report.rs"]
mod report_support;
#[path = "support/spawn.rs"]
mod spawn_support;

use anyhow::Result;
use clap::Parser;
use lilo_rm_core::{EventBatch, RuntimeKind, SpawnTarget};
use uuid::Uuid;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    runtime: RuntimeKind,
    #[arg(long)]
    session_id: Uuid,
    #[arg(long, value_name = "headless|tmux:SESSION:WINDOW.PANE")]
    target: SpawnTarget,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let socket_path = lilo_runtime_app::shared::socket_path()?;
    let response =
        spawn_support::spawn_runtime(&socket_path, args.session_id, args.runtime, args.target)
            .await?;
    let events = lilo_runtime_app::shared::events(&socket_path, None, None).await?;

    report_support::print_spawned(response)?;
    println!("runtime events observed={}", event_count(&events));
    Ok(())
}

fn event_count(events: &EventBatch) -> usize {
    if let EventBatch::Events { events, .. } = events {
        events.len()
    } else {
        0
    }
}
