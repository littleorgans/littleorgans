use anyhow::{Context, Result, bail};
use clap::Parser;
use lilo_runtime_app::cli::{Cli, output};
use uuid::Uuid;

fn main() -> Result<()> {
    if let Some(session_id) = shim_session_id()? {
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

fn shim_session_id() -> Result<Option<Uuid>> {
    let mut args = std::env::args_os();
    let _bin = args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("__runtime-shim")) {
        return Ok(None);
    }

    let flag = args
        .next()
        .context("__runtime-shim requires --session-id")?;
    if flag != "--session-id" {
        bail!(
            "__runtime-shim expects --session-id, got {}",
            flag.to_string_lossy()
        );
    }
    let session_id = args
        .next()
        .context("__runtime-shim requires a session id")?
        .into_string()
        .map_err(|value| {
            anyhow::anyhow!("invalid unicode session id: {}", value.to_string_lossy())
        })?
        .parse()
        .context("invalid shim session id")?;
    Ok(Some(session_id))
}
