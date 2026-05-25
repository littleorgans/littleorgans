pub mod doctor;

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use lilo_common::diagnostic::Diagnostic;

use self::doctor::DoctorCommand;

#[derive(Debug, Parser)]
#[command(
    name = "lilo",
    version = lilo_common::version::VERSION_STRING,
    about = "Local-first Little Organs control plane",
    arg_required_else_help = true,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[arg(long, global = true, value_enum, default_value_t = Output::Human)]
    output: Output,
    #[arg(long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

impl Cli {
    pub fn output(&self) -> Output {
        self.output
    }

    pub fn run(&self) -> Result<(), Diagnostic> {
        match &self.command {
            Command::Doctor(command) => command.run(self.output),
            command => Err(command.not_implemented()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Output {
    Human,
    Json,
}

impl Output {
    pub fn write_diagnostic(self, diagnostic: &Diagnostic) {
        match self {
            Self::Human => eprintln!("{diagnostic}"),
            Self::Json => match serde_json::to_string(diagnostic) {
                Ok(json) => eprintln!("{json}"),
                Err(error) => eprintln!("internal: failed to serialize diagnostic: {error}"),
            },
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Inspect local lilo health")]
    Doctor(DoctorCommand),
    Run(PlaceholderArgs),
    Create(PlaceholderArgs),
    Get(PlaceholderArgs),
    Delete(PlaceholderArgs),
    Label(PlaceholderArgs),
    Mail(PlaceholderArgs),
    Nudge(PlaceholderArgs),
    Capture(PlaceholderArgs),
    Logs(PlaceholderArgs),
    Wait(PlaceholderArgs),
    Mcp(PlaceholderArgs),
    #[command(
        about = "Raw runtime operator namespace. runtime spawn never creates session records."
    )]
    Runtime(PlaceholderArgs),
    Session(PlaceholderArgs),
    Identity(PlaceholderArgs),
    Daemon(PlaceholderArgs),
    #[command(name = "__runtime-shim", hide = true)]
    RuntimeShim(PlaceholderArgs),
}

impl Command {
    fn not_implemented(&self) -> Diagnostic {
        Diagnostic::domain(format!("{} is not yet implemented", self.name()))
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Doctor(_) => "doctor",
            Self::Run(_) => "run",
            Self::Create(_) => "create",
            Self::Get(_) => "get",
            Self::Delete(_) => "delete",
            Self::Label(_) => "label",
            Self::Mail(_) => "mail",
            Self::Nudge(_) => "nudge",
            Self::Capture(_) => "capture",
            Self::Logs(_) => "logs",
            Self::Wait(_) => "wait",
            Self::Mcp(_) => "mcp",
            Self::Runtime(_) => "runtime",
            Self::Session(_) => "session",
            Self::Identity(_) => "identity",
            Self::Daemon(_) => "daemon",
            Self::RuntimeShim(_) => "__runtime-shim",
        }
    }
}

#[derive(Debug, Args)]
pub struct PlaceholderArgs {
    #[arg(num_args = 0.., trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub args: Vec<OsString>,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn help_lists_public_commands_and_hides_runtime_shim() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("doctor"));
        assert!(help.contains("runtime"));
        assert!(!help.contains("__runtime-shim"));
    }

    #[test]
    fn output_flag_is_global_after_subcommands() {
        let cli = Cli::try_parse_from(["lilo", "doctor", "--output", "json"])
            .expect("parse doctor json output");

        assert_eq!(cli.output(), Output::Json);
    }

    #[test]
    fn placeholder_commands_accept_future_arguments() {
        let cli = Cli::try_parse_from(["lilo", "runtime", "spawn", "--raw"])
            .expect("parse future runtime args");

        let error = cli.run().expect_err("runtime is not implemented");

        assert_eq!(error.exit_code, lilo_common::exit_codes::DOMAIN);
        assert!(error.message.contains("runtime is not yet implemented"));
    }
}
