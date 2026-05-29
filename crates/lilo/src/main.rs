mod cli;

use std::process::ExitCode;

use clap::Parser;
use lilo_common::{diagnostic::Diagnostic, exit_codes, logging};

use crate::cli::Cli;

pub const VERSION: &str = env!("LILO_CLI_VERSION");

fn main() -> ExitCode {
    match lilo_runtime_app::cli::shim::runtime_shim_session_id_from_env() {
        Ok(Some(session_id)) => {
            return exit_result(lilo_runtime_app::cli::shim::run_for_session_blocking(
                session_id,
            ));
        }
        Ok(None) => {}
        Err(error) => return exit_result(Err(error)),
    }

    let cli = Cli::parse();
    let output = cli.output();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            output.write_diagnostic(
                &Diagnostic::internal("failed to build tokio runtime")
                    .with_detail(error.to_string()),
            );
            return exit_code(exit_codes::INTERNAL);
        }
    };

    match match logging::init_logging() {
        Ok(()) => runtime.block_on(cli.run()),
        Err(error) => Err(error),
    } {
        // Exit codes are `i32` to align with `Diagnostic.exit_code`, but fit
        // in `u8` by construction; fall back to 1 (`INTERNAL`) defensively.
        Ok(()) => exit_code(exit_codes::SUCCESS),
        Err(diagnostic) => {
            output.write_diagnostic(&diagnostic);
            exit_code(diagnostic.exit_code)
        }
    }
}

fn exit_result(result: anyhow::Result<()>) -> ExitCode {
    match result {
        Ok(()) => exit_code(exit_codes::SUCCESS),
        Err(error) => {
            eprintln!("{error:#}");
            exit_code(exit_codes::INTERNAL)
        }
    }
}

fn exit_code(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
