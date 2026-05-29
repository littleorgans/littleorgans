#![cfg_attr(not(test), forbid(unsafe_code))]

pub mod cli;
pub mod compose;
pub mod mcp;
pub mod tool_contracts;
pub mod tool_docs;
pub mod tool_examples;

#[cfg(test)]
#[path = "../../test_support.rs"]
mod test_support;

use clap::Parser;

use cli::cli_def::Cli;

pub const VERSION: &str = env!("SM_CLI_VERSION");

pub async fn run() -> anyhow::Result<()> {
    if render_bare_leaf_help()? {
        return Ok(());
    }

    cli::dispatch(Cli::parse().command, false).await
}

fn render_bare_leaf_help() -> anyhow::Result<bool> {
    let mut args = std::env::args_os();
    let Some(_) = args.next() else {
        return Ok(false);
    };
    let Some(command_name) = args.next().and_then(|arg| arg.into_string().ok()) else {
        return Ok(false);
    };
    if args.next().is_some() || !BARE_HELP_LEAF_COMMANDS.contains(&command_name.as_str()) {
        return Ok(false);
    }

    match Cli::try_parse_from(["sm", command_name.as_str(), "--help"]) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == clap::error::ErrorKind::DisplayHelp => {
            error.print()?;
            Ok(true)
        }
        Err(error) => Err(error.into()),
    }
}

const BARE_HELP_LEAF_COMMANDS: &[&str] = &["label", "logs", "capture", "wait", "nudge", "run"];
