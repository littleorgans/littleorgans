mod cli;

use std::process::ExitCode;

use clap::Parser;
use lilo_common::{exit_codes, logging};

use crate::cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();

    match logging::init_logging().and_then(|_| cli.run()) {
        Ok(()) => ExitCode::from(exit_codes::SUCCESS as u8),
        Err(diagnostic) => {
            cli.output().write_diagnostic(&diagnostic);
            ExitCode::from(diagnostic.exit_code as u8)
        }
    }
}
