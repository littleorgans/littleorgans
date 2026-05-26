mod cli;

use std::process::ExitCode;

use clap::Parser;
use lilo_common::{exit_codes, logging};

use crate::cli::Cli;

pub const VERSION: &str = env!("LILO_CLI_VERSION");

fn main() -> ExitCode {
    let cli = Cli::parse();

    match logging::init_logging().and_then(|()| cli.run()) {
        // Exit codes are `i32` to align with `Diagnostic.exit_code`, but fit
        // in `u8` by construction; fall back to 1 (`INTERNAL`) defensively.
        Ok(()) => ExitCode::from(u8::try_from(exit_codes::SUCCESS).unwrap_or(1)),
        Err(diagnostic) => {
            cli.output().write_diagnostic(&diagnostic);
            ExitCode::from(u8::try_from(diagnostic.exit_code).unwrap_or(1))
        }
    }
}
