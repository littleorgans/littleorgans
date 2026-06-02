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
    #[arg(
        long,
        global = true,
        action = clap::ArgAction::Count,
        help = "Increase log verbosity; repeat for more detail (-v, -vv)."
    )]
    pub verbose: u8,
    #[arg(long, short = 'c', global = true)]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

impl Cli {
    pub fn output(&self) -> Output {
        self.output
    }

    pub async fn run(self) -> Result<(), Diagnostic> {
        let output = self.output;
        let json_output = output == Output::Json;
        if self.config.is_some() {
            return Err(unsupported_config_file());
        }

        match self.command {
            Command::Run(args) => {
                run_session(session_cli_def::Command::Run(args), json_output).await
            }
            Command::Create(args) => {
                run_session(session_cli_def::Command::Create(args), json_output).await
            }
            Command::Get(args) => {
                run_session(session_cli_def::Command::Get(args), json_output).await
            }
            Command::Delete(args) => {
                run_session(session_cli_def::Command::Delete(args), json_output).await
            }
            Command::Label(args) => {
                run_session(session_cli_def::Command::Label(args), json_output).await
            }
            Command::Mail(args) => {
                run_session(session_cli_def::Command::Mail(args), json_output).await
            }
            Command::Nudge(args) => {
                run_session(session_cli_def::Command::Nudge(args), json_output).await
            }
            Command::Capture(args) => {
                run_session(session_cli_def::Command::Capture(args), json_output).await
            }
            Command::Logs(args) => {
                run_session(session_cli_def::Command::Logs(args), json_output).await
            }
            Command::Wait(args) => {
                run_session(session_cli_def::Command::Wait(args), json_output).await
            }
            Command::Mcp(args) => {
                run_session(session_cli_def::Command::Mcp(args), json_output).await
            }
            Command::Runtime(args) => {
                reject_unsupported_json_output("runtime", json_output)?;
                runtime_cli::run_operator(args)
                    .await
                    .map_err(Diagnostic::from)
            }
            Command::Session(args) => run_session_operator(args, json_output).await,
            Command::Doctor(command) => command.run(self.output).await,
            Command::Daemon(command) => command.run(self.output).await,
            Command::RuntimeShim(args) => {
                reject_unsupported_json_output("__shim", json_output)?;
                lilo_runtime_app::cli::shim::run(args)
                    .await
                    .map_err(Diagnostic::from)
            }
            Command::Identity(args) => {
                reject_unsupported_json_output(
                    "identity",
                    json_output || args.requests_json_output(),
                )?;
                Err(Diagnostic::domain("identity is not yet implemented"))
            }
        }
    }
}

async fn run_session(
    command: session_cli_def::Command,
    json_output: bool,
) -> Result<(), Diagnostic> {
    validate_session_json_output(&command, json_output)?;
    session_cli::dispatch(command, json_output)
        .await
        .map_err(Diagnostic::from)
}

async fn run_session_operator(
    args: session_cli::OperatorArgs,
    json_output: bool,
) -> Result<(), Diagnostic> {
    validate_session_json_output(&args.command, json_output)?;
    session_cli::run_operator(args, json_output)
        .await
        .map_err(Diagnostic::from)
}

fn validate_session_json_output(
    command: &session_cli_def::Command,
    json_output: bool,
) -> Result<(), Diagnostic> {
    if let (true, session_cli::JsonOutputSupport::Unsupported(command)) =
        (json_output, command.json_output_support())
    {
        return Err(unsupported_json_output(command));
    }

    Ok(())
}

fn reject_unsupported_json_output(command: &str, json_output: bool) -> Result<(), Diagnostic> {
    if json_output {
        return Err(unsupported_json_output(command));
    }

    Ok(())
}

fn unsupported_config_file() -> Diagnostic {
    Diagnostic::input_validation(
        "--config/-c is not supported; lilo configuration is environment only. Use LILO_HOME, LILO_SOCKET_PATH, or LILO_LOG.",
    )
}

fn unsupported_json_output(command: &str) -> Diagnostic {
    Diagnostic::input_validation(format!("--output json is not supported for `{command}`"))
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

/// Leaf help layout for commands that carry an Examples block: the
/// `{after-help}` block renders above the arguments/options section, the
/// kubectl order (teaching before reference), rather than clap's default
/// bottom position. Applied only to commands with examples; commands without
/// keep clap's default template so their spacing stays clean.
const LEAF_HELP_TEMPLATE: &str =
    "{before-help}{about-with-newline}\n{usage-heading} {usage}{after-help}\n{all-args}";

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
    #[command(
        help_template = LEAF_HELP_TEMPLATE,
        about = generated_help::RUN_ABOUT,
        long_about = generated_help::RUN_LONG_ABOUT,
        after_help = generated_help::RUN_EXAMPLES
    )]
    Run(session_cli_def::RunArgs) => "run",
    #[command(
        help_template = LEAF_HELP_TEMPLATE,
        about = generated_help::CREATE_ABOUT,
        long_about = generated_help::CREATE_LONG_ABOUT,
        after_help = generated_help::CREATE_EXAMPLES
    )]
    Create(session_cli_def::CreateArgs) => "create",
    #[command(
        help_template = LEAF_HELP_TEMPLATE,
        about = generated_help::GET_ABOUT,
        long_about = generated_help::GET_LONG_ABOUT,
        after_help = generated_help::GET_EXAMPLES
    )]
    Get(session_cli_def::GetArgs) => "get",
    #[command(
        help_template = LEAF_HELP_TEMPLATE,
        about = generated_help::DELETE_ABOUT,
        long_about = generated_help::DELETE_LONG_ABOUT,
        after_help = generated_help::DELETE_EXAMPLES
    )]
    Delete(session_cli_def::DeleteArgs) => "delete",
    #[command(
        help_template = LEAF_HELP_TEMPLATE,
        about = generated_help::LABEL_ABOUT,
        long_about = generated_help::LABEL_LONG_ABOUT,
        after_help = generated_help::LABEL_EXAMPLES
    )]
    Label(session_cli_def::LabelArgs) => "label",
    #[command(
        help_template = LEAF_HELP_TEMPLATE,
        about = generated_help::MAIL_ABOUT,
        long_about = generated_help::MAIL_LONG_ABOUT,
        after_help = generated_help::MAIL_EXAMPLES
    )]
    Mail(session_cli_def::MailArgs) => "mail",
    #[command(
        help_template = LEAF_HELP_TEMPLATE,
        about = generated_help::NUDGE_ABOUT,
        long_about = generated_help::NUDGE_LONG_ABOUT,
        after_help = generated_help::NUDGE_EXAMPLES
    )]
    Nudge(session_cli_def::NudgeArgs) => "nudge",
    #[command(
        help_template = LEAF_HELP_TEMPLATE,
        about = generated_help::CAPTURE_ABOUT,
        long_about = generated_help::CAPTURE_LONG_ABOUT,
        after_help = generated_help::CAPTURE_EXAMPLES
    )]
    Capture(session_cli_def::CaptureArgs) => "capture",
    #[command(
        help_template = LEAF_HELP_TEMPLATE,
        about = generated_help::LOGS_ABOUT,
        long_about = generated_help::LOGS_LONG_ABOUT,
        after_help = generated_help::LOGS_EXAMPLES
    )]
    Logs(session_cli_def::LogsArgs) => "logs",
    #[command(
        help_template = LEAF_HELP_TEMPLATE,
        about = generated_help::WAIT_ABOUT,
        long_about = generated_help::WAIT_LONG_ABOUT,
        after_help = generated_help::WAIT_EXAMPLES
    )]
    Wait(session_cli_def::WaitArgs) => "wait",
    #[command(
        help_template = LEAF_HELP_TEMPLATE,
        about = generated_help::MCP_ABOUT,
        long_about = generated_help::MCP_LONG_ABOUT,
        after_help = generated_help::MCP_EXAMPLES
    )]
    Mcp(session_cli_def::McpArgs) => "mcp",
    #[command(about = generated_help::RUNTIME_ABOUT)]
    Runtime(runtime_cli::OperatorArgs) => "runtime",
    #[command(about = generated_help::SESSION_ABOUT)]
    Session(session_cli::OperatorArgs) => "session",
    #[command(about = generated_help::IDENTITY_ABOUT)]
    Identity(PlaceholderArgs) => "identity",
    #[command(about = generated_help::DOCTOR_ABOUT)]
    Doctor(DoctorCommand) => "doctor",
    #[command(about = generated_help::DAEMON_ABOUT)]
    Daemon(DaemonCli) => "daemon",
    #[command(hide = true)]
    RuntimeShim(lilo_runtime_app::cli::shim::ShimArgs) => "__shim",
);

#[derive(Debug, Args)]
pub struct PlaceholderArgs {
    #[arg(num_args = 0.., trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub args: Vec<OsString>,
}

impl PlaceholderArgs {
    fn requests_json_output(&self) -> bool {
        self.args.iter().any(|arg| arg == "--output=json")
            || self
                .args
                .windows(2)
                .any(|args| args[0] == "--output" && args[1] == "json")
    }
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
        assert!(!help.contains("__shim"));
    }

    #[test]
    fn run_help_uses_options_heading_and_shows_examples() {
        let mut command = Cli::command();
        let run = command
            .find_subcommand_mut("run")
            .expect("run subcommand exists");
        let help = run.render_long_help().to_string();

        assert!(
            !help.contains("Session commands:"),
            "run flags must not sit under the root group heading: {help}"
        );
        assert!(
            help.contains("Examples:"),
            "run --help must include an Examples block: {help}"
        );
        assert!(
            help.contains("lilo run --role engineer --dir . claude"),
            "run --help must show the headless example: {help}"
        );
    }

    // Scope: clap-parse validity only — flag existence, required args, and
    // FromStr-flag syntax (the isolation enum, MountSpec). It does NOT validate
    // runtime semantics: selectors and `--target` are plain strings, and
    // `docker:PROFILE` accepts any name, so example semantics (real selectors,
    // valid tmux targets, real docker profiles) are gated by review, not by
    // this test.
    #[test]
    fn documented_examples_parse_as_valid_invocations() {
        for args in generated_help::EXAMPLE_INVOCATIONS {
            let argv = std::iter::once("lilo").chain(args.iter().copied());
            Cli::try_parse_from(argv).unwrap_or_else(|error| {
                panic!(
                    "documented example failed to parse: lilo {}: {error}",
                    args.join(" ")
                )
            });
        }
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

    #[tokio::test]
    async fn config_file_flag_is_rejected_before_dispatch() {
        for flag in ["--config", "-c"] {
            let cli = Cli::try_parse_from(["lilo", flag, "x.toml", "doctor"])
                .unwrap_or_else(|error| panic!("parse {flag}: {error}"));

            let error = match cli.run().await {
                Ok(()) => panic!("{flag} unexpectedly succeeded"),
                Err(error) => error,
            };

            assert_eq!(error.code, "input_validation");
            assert_ne!(error.exit_code, 0);
            for expected in ["--config/-c", "LILO_HOME", "LILO_SOCKET_PATH", "LILO_LOG"] {
                assert!(
                    error.message.contains(expected),
                    "{flag} returned unexpected message: {}",
                    error.message
                );
            }
        }
    }

    #[test]
    fn config_file_flag_absent_leaves_config_unset() {
        let cli = Cli::try_parse_from(["lilo", "doctor"]).expect("parse doctor without config");

        assert!(cli.config.is_none());
    }

    #[test]
    fn capture_accepts_global_output_and_rejects_json_flag() {
        let id = "018f6e28-0000-7000-8000-000000000001";
        let cli = Cli::try_parse_from(["lilo", "capture", id, "--output", "json"])
            .expect("parse capture json output");

        assert_eq!(cli.output(), Output::Json);
        Cli::try_parse_from(["lilo", "capture", id, "--json"])
            .expect_err("capture --json is not a retained CLI surface");
    }

    #[test]
    fn get_accepts_global_output_and_rejects_json_flag() {
        for resource in ["session", "namespace"] {
            let cli = Cli::try_parse_from(["lilo", "get", resource, "--output", "json"])
                .expect("parse get json output");

            assert_eq!(cli.output(), Output::Json);
            Cli::try_parse_from(["lilo", "get", resource, "--json"])
                .expect_err("get --json is not a retained CLI surface");
        }
    }

    #[tokio::test]
    async fn unsupported_global_json_output_is_rejected_before_dispatch() {
        let cases: &[(&[&str], &str)] = &[
            (
                &[
                    "lilo", "run", "claude", "--role", "engineer", "--dir", ".", "--output", "json",
                ],
                "run",
            ),
            (
                &["lilo", "create", "namespace", "alpha", "--output", "json"],
                "create",
            ),
            (
                &["lilo", "delete", "session", "abc", "--output", "json"],
                "delete",
            ),
            (
                &["lilo", "label", "abc", "key=value", "--output", "json"],
                "label",
            ),
            (
                &[
                    "lilo",
                    "nudge",
                    "--to",
                    "abc",
                    "--content",
                    "hello",
                    "--output",
                    "json",
                ],
                "nudge",
            ),
            (&["lilo", "logs", "abc", "--output", "json"], "logs"),
            (
                &["lilo", "wait", "abc", "--for", "done", "--output", "json"],
                "wait",
            ),
            (&["lilo", "mcp", "--output", "json"], "mcp"),
            (
                &[
                    "lilo",
                    "session",
                    "label",
                    "abc",
                    "key=value",
                    "--output",
                    "json",
                ],
                "label",
            ),
            (
                &["lilo", "runtime", "status", "--output", "json"],
                "runtime",
            ),
            (
                &["lilo", "identity", "whoami", "--output", "json"],
                "identity",
            ),
        ];

        for (args, command) in cases {
            let cli = Cli::try_parse_from(args.iter().copied())
                .unwrap_or_else(|error| panic!("parse {}: {error}", args.join(" ")));

            let error = match cli.run().await {
                Ok(()) => panic!("{} unexpectedly succeeded", args.join(" ")),
                Err(error) => error,
            };

            assert_eq!(error.code, "input_validation");
            assert!(
                error
                    .message
                    .contains(&format!("--output json is not supported for `{command}`")),
                "{} returned unexpected message: {}",
                args.join(" "),
                error.message
            );
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
