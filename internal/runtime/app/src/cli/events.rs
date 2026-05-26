use anyhow::{Result, bail};
use clap::Args;
use lilo_rm_core::{EventBatch, EventsPayload};
use serde::Serialize;

use crate::cli::output;

const CURSOR_EXPIRED_EXIT_CODE: i32 = 2;

#[derive(Debug, Args)]
pub struct EventsArgs {
    #[command(flatten)]
    output: output::OutputArgs,
    #[arg(long)]
    since: Option<lilo_rm_core::EventCursor>,
    #[arg(long)]
    wait_ms: Option<u32>,
}

pub async fn run(args: EventsArgs) -> Result<()> {
    let socket_path = crate::shared::socket_path()?;
    let batch = crate::shared::events(&socket_path, args.since, args.wait_ms).await?;
    match batch {
        EventBatch::Events { events, cursor } => {
            output::emit(&args.output, &EventsPayload { events, cursor })?;
        }
        EventBatch::CursorExpired { oldest } => emit_cursor_expired(&args.output, oldest)?,
        _ => bail!("unexpected events batch"),
    }
    Ok(())
}

fn emit_cursor_expired(
    args: &output::OutputArgs,
    latest_cursor: lilo_rm_core::EventCursor,
) -> Result<()> {
    match args.format {
        output::OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&CursorExpiredOutput {
                    cursor_expired: true,
                    latest_cursor,
                })?
            );
        }
        output::OutputFormat::Human => {
            eprintln!("cursor expired (latest_cursor: {latest_cursor})");
        }
    }
    std::process::exit(CURSOR_EXPIRED_EXIT_CODE);
}

#[derive(Serialize)]
struct CursorExpiredOutput {
    cursor_expired: bool,
    latest_cursor: lilo_rm_core::EventCursor,
}
