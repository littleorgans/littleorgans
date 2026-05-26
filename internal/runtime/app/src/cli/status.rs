use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Args;
use lilo_rm_core::{RuntimeResponse, StatusFilter};
use uuid::Uuid;

use crate::cli::output;

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[command(flatten)]
    output: output::OutputArgs,
    #[arg(long = "session-id", value_name = "UUID")]
    session_ids: Vec<Uuid>,
    #[arg(long, value_parser = parse_updated_since)]
    updated_since: Option<DateTime<Utc>>,
    #[arg(long)]
    runtime: Option<String>,
    #[arg(long)]
    state: Option<String>,
}

pub async fn run(args: StatusArgs) -> Result<()> {
    let socket_path = crate::shared::socket_path()?;
    let response = crate::shared::status_filtered(
        &socket_path,
        StatusFilter {
            session_id: None,
            session_ids: args.session_ids,
            updated_since: args.updated_since,
            runtime: args.runtime,
            state: args.state,
        },
    )
    .await?;
    match response {
        RuntimeResponse::Status(payload) => output::emit(&args.output, &payload.lifecycles)?,
        other => anyhow::bail!("unexpected status response: {other:?}"),
    }
    Ok(())
}

fn parse_updated_since(value: &str) -> std::result::Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|time| time.with_timezone(&Utc))
}
