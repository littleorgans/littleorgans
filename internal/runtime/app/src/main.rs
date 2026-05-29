use anyhow::{Context, Result};
use clap::Parser;
use lilo_runtime_app::cli::{Cli, output};

fn main() -> Result<()> {
    if let Some(session_id) = lilo_runtime_app::cli::shim::runtime_shim_session_id_from_env()? {
        return lilo_runtime_app::cli::shim::run_for_session_blocking(session_id);
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")?;

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let format = output::requested_format_from_env();
    if let Err(error) = runtime.block_on(Cli::parse().run()) {
        output::emit_error(format, &error)?;
        std::process::exit(1);
    }
    Ok(())
}
