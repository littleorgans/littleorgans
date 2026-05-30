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

pub async fn run_operator(args: OperatorArgs, json_output: bool) -> Result<()> {
    dispatch(args.command, json_output).await
}

pub async fn dispatch(command: Command, json_output: bool) -> Result<()> {
    command.json_output_support().ensure(json_output)?;

    match command {
        Command::Run(args) => run::run(args).await,
        Command::Create(args) => namespace::create(args).await,
        Command::Config(args) => config::run(args).await,
        Command::Get(args) => get::run(args, json_output).await,
        Command::Delete(args) => delete::run(args).await,
        Command::Doctor(args) => doctor::run(args).await,
        Command::Mail(args) => mail::run(args).await,
        Command::Label(args) => label::run(args).await,
        Command::Logs(args) => logs::run(args).await,
        Command::Capture(args) => capture::run(args, json_output).await,
        Command::Wait(args) => wait::run(args).await,
        Command::Nudge(args) => nudge::run(args).await,
        Command::Mcp(args) => mcp::run(args).await,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonOutputSupport {
    Supported,
    Unsupported(&'static str),
}

impl JsonOutputSupport {
    fn ensure(self, requested: bool) -> Result<()> {
        if let (true, Self::Unsupported(command)) = (requested, self) {
            anyhow::bail!("--output json is not supported for `{command}`");
        }

        Ok(())
    }
}

impl Command {
    pub fn json_output_support(&self) -> JsonOutputSupport {
        match self {
            Self::Get(_) | Self::Capture(_) => JsonOutputSupport::Supported,
            Self::Run(_) => JsonOutputSupport::Unsupported("run"),
            Self::Create(_) => JsonOutputSupport::Unsupported("create"),
            Self::Config(_) => JsonOutputSupport::Unsupported("config"),
            Self::Delete(_) => JsonOutputSupport::Unsupported("delete"),
            Self::Doctor(_) => JsonOutputSupport::Unsupported("doctor"),
            Self::Mail(_) => JsonOutputSupport::Unsupported("mail"),
            Self::Label(_) => JsonOutputSupport::Unsupported("label"),
            Self::Logs(_) => JsonOutputSupport::Unsupported("logs"),
            Self::Wait(_) => JsonOutputSupport::Unsupported("wait"),
            Self::Nudge(_) => JsonOutputSupport::Unsupported("nudge"),
            Self::Mcp(_) => JsonOutputSupport::Unsupported("mcp"),
        }
    }
}
