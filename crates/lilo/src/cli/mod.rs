pub mod daemon;
pub mod doctor;
pub mod generated_help;
pub mod generated_schema;

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use lilo_common::diagnostic::Diagnostic;
use lilo_paths::{LiloHome, LiloPathError, LiloPaths};
use lilo_runtime_app::cli as runtime_cli;
use lilo_session_app::cli::{self as session_cli, cli_def as session_cli_def};

use self::{daemon::DaemonCli, doctor::DoctorCommand};

#[derive(Debug, Parser)]
#[command(
    name = "lilo",
    display_name = "littleorgans",
    version = crate::VERSION,
    about = "Local-first Little Organs control plane",
    arg_required_else_help = true,
    disable_help_subcommand = true,
    help_template = generated_help::ROOT_HELP_TEMPLATE
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

    pub async fn run(self) -> Result<(), Diagnostic> {
        match self.command {
            Command::Run(args) => run_session(session_cli_def::Command::Run(args)).await,
            Command::Create(args) => run_session(session_cli_def::Command::Create(args)).await,
            Command::Get(args) => run_session(session_cli_def::Command::Get(args)).await,
            Command::Delete(args) => run_session(session_cli_def::Command::Delete(args)).await,
            Command::Label(args) => run_session(session_cli_def::Command::Label(args)).await,
            Command::Mail(args) => run_session(session_cli_def::Command::Mail(args)).await,
            Command::Nudge(args) => run_session(session_cli_def::Command::Nudge(args)).await,
            Command::Capture(args) => run_session(session_cli_def::Command::Capture(args)).await,
            Command::Logs(args) => run_session(session_cli_def::Command::Logs(args)).await,
            Command::Wait(args) => run_session(session_cli_def::Command::Wait(args)).await,
            Command::Mcp(args) => run_session(session_cli_def::Command::Mcp(args)).await,
            Command::Runtime(args) => runtime_cli::run_operator(args)
                .await
                .map_err(Diagnostic::from),
            Command::Session(args) => session_cli::run_operator(args)
                .await
                .map_err(Diagnostic::from),
            Command::Doctor(command) => command.run(self.output).await,
            Command::Daemon(command) => command.run(self.output).await,
            Command::RuntimeShim(args) => lilo_runtime_app::cli::shim::run(args)
                .await
                .map_err(Diagnostic::from),
            Command::Identity(_) => Err(Diagnostic::domain("identity is not yet implemented")),
        }
    }
}

async fn run_session(command: session_cli_def::Command) -> Result<(), Diagnostic> {
    session_cli::dispatch(command)
        .await
        .map_err(Diagnostic::from)
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

pub(crate) fn resolve_lilo_paths() -> Result<LiloPaths, LiloPathError> {
    let home = LiloHome::from_env()?;
    Ok(LiloPaths::new(home))
}

macro_rules! define_commands {
    ($(
        $(#[$meta:meta])*
        $variant:ident($payload:ty) => $name:literal
    ),+ $(,)?) => {
        #[derive(Debug, Subcommand)]
        pub enum Command {
            $(
                #[command(name = $name)]
                $(#[$meta])*
                $variant($payload),
            )+
        }
    };
}

define_commands!(
    #[command(next_help_heading = "Session commands", about = generated_help::RUN_ABOUT)]
    Run(session_cli_def::RunArgs) => "run",
    #[command(
        next_help_heading = "Session commands",
        about = generated_help::CREATE_ABOUT
    )]
    Create(session_cli_def::CreateArgs) => "create",
    #[command(next_help_heading = "Session commands", about = generated_help::GET_ABOUT)]
    Get(session_cli_def::GetArgs) => "get",
    #[command(
        next_help_heading = "Session commands",
        about = generated_help::DELETE_ABOUT
    )]
    Delete(session_cli_def::DeleteArgs) => "delete",
    #[command(next_help_heading = "Session commands", about = generated_help::LABEL_ABOUT)]
    Label(session_cli_def::LabelArgs) => "label",
    #[command(next_help_heading = "Session commands", about = generated_help::MAIL_ABOUT)]
    Mail(session_cli_def::MailArgs) => "mail",
    #[command(next_help_heading = "Session commands", about = generated_help::NUDGE_ABOUT)]
    Nudge(session_cli_def::NudgeArgs) => "nudge",
    #[command(next_help_heading = "Session commands", about = generated_help::CAPTURE_ABOUT)]
    Capture(session_cli_def::CaptureArgs) => "capture",
    #[command(next_help_heading = "Session commands", about = generated_help::LOGS_ABOUT)]
    Logs(session_cli_def::LogsArgs) => "logs",
    #[command(next_help_heading = "Session commands", about = generated_help::WAIT_ABOUT)]
    Wait(session_cli_def::WaitArgs) => "wait",
    #[command(next_help_heading = "Session commands", about = generated_help::MCP_ABOUT)]
    Mcp(session_cli_def::McpArgs) => "mcp",
    #[command(
        next_help_heading = "Substrate operator commands",
        about = generated_help::RUNTIME_ABOUT
    )]
    Runtime(runtime_cli::OperatorArgs) => "runtime",
    #[command(
        next_help_heading = "Substrate operator commands",
        about = generated_help::SESSION_ABOUT
    )]
    Session(session_cli::OperatorArgs) => "session",
    #[command(
        next_help_heading = "Substrate operator commands",
        about = generated_help::IDENTITY_ABOUT
    )]
    Identity(PlaceholderArgs) => "identity",
    #[command(next_help_heading = "Diagnostics", about = generated_help::DOCTOR_ABOUT)]
    Doctor(DoctorCommand) => "doctor",
    #[command(next_help_heading = "Daemon lifecycle", about = generated_help::DAEMON_ABOUT)]
    Daemon(DaemonCli) => "daemon",
    #[command(hide = true)]
    RuntimeShim(lilo_runtime_app::cli::shim::ShimArgs) => "__runtime-shim",
);

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
    fn help_groups_commands_by_category() {
        let help = Cli::command().render_long_help().to_string();

        for heading in [
            "Session commands:",
            "Substrate operator commands:",
            "Diagnostics:",
            "Daemon lifecycle:",
        ] {
            assert!(help.contains(heading), "missing heading: {heading}");
        }

        let session_idx = help.find("Session commands:").unwrap();
        let operator_idx = help.find("Substrate operator commands:").unwrap();
        let diagnostics_idx = help.find("Diagnostics:").unwrap();
        let daemon_idx = help.find("Daemon lifecycle:").unwrap();
        assert!(session_idx < operator_idx);
        assert!(operator_idx < diagnostics_idx);
        assert!(diagnostics_idx < daemon_idx);
    }

    #[test]
    fn output_flag_is_global_after_subcommands() {
        let cli = Cli::try_parse_from(["lilo", "doctor", "--output", "json"])
            .expect("parse doctor json output");

        assert_eq!(cli.output(), Output::Json);
    }

    #[test]
    fn operator_namespaces_have_w3_subcommands() {
        let session_id = "018f6e28-0000-7000-8000-000000000001";

        for args in [
            vec![
                "lilo",
                "runtime",
                "spawn",
                "--runtime",
                "claude",
                "--session-id",
                session_id,
                "--target",
                "headless",
            ],
            vec!["lilo", "runtime", "status", "--session-id", session_id],
            vec!["lilo", "runtime", "events"],
            vec!["lilo", "runtime", "kill", session_id],
            vec!["lilo", "session", "config", "set-context", "default"],
            vec!["lilo", "__runtime-shim", "--session-id", session_id],
        ] {
            Cli::try_parse_from(args).expect("operator command parses");
        }
    }

    #[tokio::test]
    async fn placeholder_commands_accept_future_arguments() {
        let cli = Cli::try_parse_from(["lilo", "identity", "whoami"])
            .expect("parse future identity args");

        let error = cli.run().await.expect_err("identity is not implemented");

        assert_eq!(error.exit_code, lilo_common::exit_codes::DOMAIN);
        assert!(error.message.contains("identity is not yet implemented"));
    }

    #[test]
    fn generated_schema_json_is_valid() {
        serde_json::from_str::<serde_json::Value>(generated_schema::CLI_SURFACE_JSON)
            .expect("CLI surface JSON is valid");
        serde_json::from_str::<serde_json::Value>(generated_schema::MCP_SCHEMA_JSON)
            .expect("MCP schema JSON is valid");
    }
}
