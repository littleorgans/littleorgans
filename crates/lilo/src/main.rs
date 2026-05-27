mod cli;

use std::process::ExitCode;

use clap::Parser;
use lilo_common::{exit_codes, logging};

use crate::cli::Cli;

pub const VERSION: &str = env!("LILO_CLI_VERSION");

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let output = cli.output();

    match match logging::init_logging() {
        Ok(()) => cli.run().await,
        Err(error) => Err(error),
    } {
        // Exit codes are `i32` to align with `Diagnostic.exit_code`, but fit
        // in `u8` by construction; fall back to 1 (`INTERNAL`) defensively.
        Ok(()) => ExitCode::from(u8::try_from(exit_codes::SUCCESS).unwrap_or(1)),
        Err(diagnostic) => {
            output.write_diagnostic(&diagnostic);
            ExitCode::from(u8::try_from(diagnostic.exit_code).unwrap_or(1))
        }
    }
}
