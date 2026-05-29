pub mod capture;
pub mod cli_def;
pub mod client;
pub mod config;
pub mod delete;
pub mod doctor;
pub mod generated_help;
pub mod get;
pub mod label;
pub mod logs;
pub mod mail;
pub mod mcp;
pub mod namespace;
pub mod namespace_resolver;
pub mod nudge;
pub mod output;
pub mod run;
pub mod selector_scope;
pub mod wait;

use anyhow::Result;
use clap::Args;

use self::cli_def::Command;

#[derive(Debug, Args)]
pub struct OperatorArgs {
    #[command(subcommand)]
    pub command: Command,
}

pub async fn run_operator(args: OperatorArgs) -> Result<()> {
    dispatch(args.command, false).await
}

pub async fn dispatch(command: Command, capture_json: bool) -> Result<()> {
    match command {
        Command::Run(args) => run::run(args).await,
        Command::Create(args) => namespace::create(args).await,
        Command::Config(args) => config::run(args).await,
        Command::Get(args) => get::run(args).await,
        Command::Delete(args) => delete::run(args).await,
        Command::Doctor(args) => doctor::run(args).await,
        Command::Mail(args) => mail::run(args).await,
        Command::Label(args) => label::run(args).await,
        Command::Logs(args) => logs::run(args).await,
        Command::Capture(args) => capture::run(args, capture_json).await,
        Command::Wait(args) => wait::run(args).await,
        Command::Nudge(args) => nudge::run(args).await,
        Command::Mcp(args) => mcp::run(args).await,
    }
}
