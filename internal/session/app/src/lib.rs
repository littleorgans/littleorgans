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

use cli::cli_def::{Cli, Command};

pub const VERSION: &str = env!("SM_CLI_VERSION");

pub async fn run() -> anyhow::Result<()> {
    if render_bare_leaf_help()? {
        return Ok(());
    }

    match Cli::parse().command {
        Command::Run(args) => cli::run::run(args).await,
        Command::Create(args) => cli::namespace::create(args).await,
        Command::Config(args) => cli::config::run(args).await,
        Command::Get(args) => cli::get::run(args).await,
        Command::Delete(args) => cli::delete::run(args).await,
        Command::Doctor(args) => cli::doctor::run(args).await,
        Command::Mail(args) => cli::mail::run(args).await,
        Command::Label(args) => cli::label::run(args).await,
        Command::Logs(args) => cli::logs::run(args).await,
        Command::Capture(args) => cli::capture::run(args).await,
        Command::Wait(args) => cli::wait::run(args).await,
        Command::Nudge(args) => cli::nudge::run(args).await,
        Command::Mcp(args) => cli::mcp::run(args).await,
    }
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
