use anyhow::Result;
use clap::Args;
use lilo_rm_client::RuntimeClient;
use lilo_rm_core::{ValidateTargetOutcome, ValidateTargetResponse};

use crate::cli::output;

#[derive(Debug, Args)]
pub struct ValidateTargetArgs {
    #[command(flatten)]
    output: output::OutputArgs,
    #[arg(value_name = "TARGET")]
    target: String,
}

pub async fn run(args: ValidateTargetArgs) -> Result<()> {
    let socket_path = crate::shared::socket_path()?;
    let response = RuntimeClient::new(socket_path)
        .validate_target(&args.target)
        .await?;
    emit_validate_target(&args.output, &args.target, &response)?;
    if !response.valid {
        std::process::exit(1);
    }
    Ok(())
}

fn emit_validate_target(
    args: &output::OutputArgs,
    target: &str,
    response: &ValidateTargetResponse,
) -> Result<()> {
    match args.format {
        output::OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(response)?);
        }
        output::OutputFormat::Human => {
            let validity = if response.valid { "valid" } else { "invalid" };
            println!(
                "{target}: {validity} ({})",
                validate_target_outcome_name(&response.outcome)
            );
        }
    }
    Ok(())
}

fn validate_target_outcome_name(outcome: &ValidateTargetOutcome) -> &'static str {
    match outcome {
        ValidateTargetOutcome::Valid => "Valid",
        ValidateTargetOutcome::InvalidTarget { .. } => "InvalidTarget",
        ValidateTargetOutcome::TmuxPaneDead { .. } => "TmuxPaneDead",
        ValidateTargetOutcome::UnsupportedTarget { .. } => "UnsupportedTarget",
    }
}
