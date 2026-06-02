use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::generated::cli_help;

pub mod capture;
pub mod doctor;
pub mod events;
pub mod initdb;
pub mod kill;
pub mod mcp;
pub mod nudge;
pub mod output;
pub mod shim;
pub mod spawn;
pub mod status;
pub mod validate_target;
pub mod version;

const SPAWN_ABOUT: &str = "Spawn a runtime process for a session.";
const KILL_ABOUT: &str = "Signal a runtime session by id, or a process by pid.";
const NUDGE_ABOUT: &str = "Deliver a nudge message to a running runtime session.";
const NUDGE_AFTER_HELP: &str =
    "Failure reasons: headless_lifecycle, session_ended, tmux_pane_dead.";
const CAPTURE_ABOUT: &str = "Capture the pane snapshot for a runtime session.";
const VALIDATE_TARGET_ABOUT: &str = "Validate a spawn target without starting a runtime.";
const EVENTS_ABOUT: &str = "Stream runtime lifecycle events.";

#[derive(Debug, Parser)]
#[command(name = "rtm")]
#[command(about = "runtime-matters host runtime control")]
#[command(display_name = "runtime-matters", version = crate::VERSION)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = SPAWN_ABOUT)]
    Spawn(spawn::SpawnArgs),
    #[command(about = KILL_ABOUT)]
    Kill(kill::KillArgs),
    #[command(about = NUDGE_ABOUT, after_help = NUDGE_AFTER_HELP)]
    Nudge(nudge::NudgeArgs),
    #[command(about = CAPTURE_ABOUT)]
    Capture(capture::CaptureArgs),
    #[command(about = VALIDATE_TARGET_ABOUT)]
    ValidateTarget(validate_target::ValidateTargetArgs),
    #[command(about = cli_help::STATUS_ABOUT)]
    Status(status::StatusArgs),
    #[command(about = cli_help::MCP_ABOUT)]
    Mcp,
    #[command(about = cli_help::VERSION_ABOUT)]
    Version(VersionArgs),
    #[command(about = "Print rtmd substrate health diagnostics.")]
    Doctor(DoctorArgs),
    #[command(about = EVENTS_ABOUT)]
    Events(events::EventsArgs),
    Initdb,
    #[command(name = "__shim", hide = true)]
    Shim(shim::ShimArgs),
}

#[derive(Debug, Args)]
pub struct OperatorArgs {
    #[command(subcommand)]
    pub command: OperatorCommand,
}

#[derive(Debug, Subcommand)]
pub enum OperatorCommand {
    #[command(about = SPAWN_ABOUT)]
    Spawn(spawn::SpawnArgs),
    #[command(about = KILL_ABOUT)]
    Kill(kill::KillArgs),
    #[command(about = NUDGE_ABOUT, after_help = NUDGE_AFTER_HELP)]
    Nudge(nudge::NudgeArgs),
    #[command(about = CAPTURE_ABOUT)]
    Capture(capture::CaptureArgs),
    #[command(about = VALIDATE_TARGET_ABOUT)]
    ValidateTarget(validate_target::ValidateTargetArgs),
    #[command(about = cli_help::STATUS_ABOUT)]
    Status(status::StatusArgs),
    #[command(about = EVENTS_ABOUT)]
    Events(events::EventsArgs),
}

#[derive(Debug, Args)]
pub struct VersionArgs {
    #[command(flatten)]
    output: output::OutputArgs,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[command(flatten)]
    output: output::OutputArgs,
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        dispatch(self.command).await
    }
}

pub async fn run_operator(args: OperatorArgs) -> Result<()> {
    match args.command {
        OperatorCommand::Spawn(args) => spawn::run(args).await,
        OperatorCommand::Kill(args) => kill::run(args).await,
        OperatorCommand::Nudge(args) => nudge::run(args).await,
        OperatorCommand::Capture(args) => capture::run(args).await,
        OperatorCommand::ValidateTarget(args) => validate_target::run(args).await,
        OperatorCommand::Status(args) => status::run(args).await,
        OperatorCommand::Events(args) => events::run(args).await,
    }
}

async fn dispatch(command: Command) -> Result<()> {
    match command {
        Command::Spawn(args) => spawn::run(args).await,
        Command::Kill(args) => kill::run(args).await,
        Command::Nudge(args) => nudge::run(args).await,
        Command::Capture(args) => capture::run(args).await,
        Command::ValidateTarget(args) => validate_target::run(args).await,
        Command::Status(args) => status::run(args).await,
        Command::Mcp => mcp::run().await,
        Command::Version(args) => version::run(args.output).await,
        Command::Doctor(args) => doctor::run(args.output).await,
        Command::Events(args) => events::run(args).await,
        Command::Initdb => initdb::run().await,
        Command::Shim(args) => shim::run(args).await,
    }
}
