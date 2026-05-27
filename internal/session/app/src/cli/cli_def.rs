use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use lilo_rm_core::{IsolationPolicy, MountSpec};
use lilo_session_core::{Namespace, RuntimeKind};

use crate::cli::generated_help;
use crate::cli::selector_scope::NamespaceScopeArgs;

const JSON_OUTPUT_HELP: &str = "Render output as JSON.";
const NAMESPACE_CREATE_HELP: &str = "Namespace slug to create.";
const NAMESPACE_CONTEXT_HELP: &str = "Namespace slug to use as the user context.";
const NAMESPACE_DELETE_HELP: &str = "Namespace slug to delete.";

#[derive(Debug, Parser)]
#[command(
    name = "sm",
    display_name = "session-matters",
    about = "session-matters control plane",
    version = crate::VERSION,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Manage the session-matters daemon")]
    Daemon(DaemonArgs),
    #[command(about = generated_help::SESSION_RUN_ABOUT, long_about = generated_help::SESSION_RUN_ABOUT, arg_required_else_help = true)]
    Run(RunArgs),
    #[command(about = "Create namespace and session records")]
    Create(CreateArgs),
    #[command(about = "Manage session-matters user configuration")]
    Config(ConfigArgs),
    #[command(about = "Inspect sessions and namespaces")]
    Get(GetArgs),
    #[command(about = "Delete sessions and namespaces")]
    Delete(DeleteArgs),
    #[command(about = generated_help::DOCTOR_ABOUT, long_about = generated_help::DOCTOR_ABOUT)]
    Doctor(DoctorArgs),
    #[command(about = "Send and read durable session mail")]
    Mail(MailArgs),
    #[command(about = generated_help::SESSION_LABEL_ABOUT, long_about = generated_help::SESSION_LABEL_ABOUT, arg_required_else_help = true)]
    Label(LabelArgs),
    #[command(about = generated_help::LOGS_ABOUT, long_about = generated_help::LOGS_ABOUT, arg_required_else_help = true)]
    Logs(LogsArgs),
    #[command(about = generated_help::SESSION_CAPTURE_ABOUT, long_about = generated_help::SESSION_CAPTURE_ABOUT, arg_required_else_help = true)]
    Capture(CaptureArgs),
    #[command(about = generated_help::WAIT_ABOUT, long_about = generated_help::WAIT_ABOUT, arg_required_else_help = true)]
    Wait(WaitArgs),
    #[command(about = generated_help::NUDGE_ABOUT, long_about = generated_help::NUDGE_ABOUT, arg_required_else_help = true)]
    Nudge(NudgeArgs),
    #[command(about = "Bridge MCP stdio to the session-matters daemon")]
    Mcp(McpArgs),
    #[command(name = "__smd", hide = true)]
    InternalDaemon,
}

#[derive(Debug, Args)]
pub struct DaemonArgs {
    #[command(subcommand)]
    pub action: DaemonAction,
}

#[derive(Debug, Subcommand)]
pub enum DaemonAction {
    #[command(about = "Start the session-matters daemon")]
    Start,
    #[command(about = "Stop the session-matters daemon")]
    Stop,
    #[command(about = "Show session-matters daemon status")]
    Status,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct RunArgs {
    #[command(flatten)]
    pub session: SessionCreateArgs,
    #[arg(long, help = generated_help::SESSION_RUN_ISOLATION_HELP)]
    pub isolation: Option<IsolationPolicy>,
    #[arg(long, help = generated_help::SESSION_RUN_IMAGE_HELP)]
    pub image: Option<String>,
    #[arg(
        long = "mount",
        value_name = "HOST:CONTAINER[:ro|:rw]",
        help = generated_help::SESSION_RUN_MOUNTS_HELP
    )]
    pub mounts: Vec<MountSpec>,
    #[arg(long, default_value = "headless", help = generated_help::SESSION_RUN_TARGET_HELP)]
    pub target: String,
    #[arg(long, help = generated_help::SESSION_RUN_FORCE_HELP)]
    pub force: bool,
    #[arg(
        long,
        help = "Return after creating the session instead of waiting on the runtime"
    )]
    pub detach: bool,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct SessionCreateArgs {
    #[arg(help = generated_help::SESSION_RUN_RUNTIME_HELP)]
    pub runtime: RuntimeKind,
    #[arg(long, help = generated_help::SESSION_RUN_ROLE_HELP)]
    pub role: String,
    #[arg(long, help = generated_help::SESSION_RUN_DIR_HELP)]
    pub dir: Option<PathBuf>,
    #[arg(long, help = generated_help::SESSION_RUN_NAMESPACE_HELP)]
    pub namespace: Option<Namespace>,
    #[arg(long = "label", help = generated_help::SESSION_RUN_LABELS_HELP)]
    pub labels: Vec<String>,
    #[arg(long = "agent-config", help = generated_help::SESSION_RUN_AGENT_CONFIG_HELP)]
    pub agent_config: Option<String>,
}

#[derive(Debug, Args)]
pub struct GetArgs {
    #[command(subcommand)]
    pub resource: GetResource,
}

#[derive(Debug, Subcommand)]
pub enum GetResource {
    #[command(about = generated_help::SESSION_LIST_ABOUT, long_about = generated_help::SESSION_LIST_ABOUT)]
    #[command(alias = "sessions")]
    Session(SessionGetArgs),
    #[command(about = generated_help::NAMESPACE_LIST_ABOUT, long_about = generated_help::NAMESPACE_LIST_ABOUT)]
    #[command(alias = "namespaces")]
    Namespace(NamespaceGetArgs),
}

#[derive(Debug, Args)]
pub struct SessionGetArgs {
    #[arg(help = generated_help::SESSION_LIST_ID_HELP)]
    pub id: Option<String>,
    #[command(flatten)]
    pub read: SessionReadArgs,
}

pub struct SessionListArgs {
    pub read: SessionReadArgs,
}

#[derive(Debug, Args)]
pub struct SessionReadArgs {
    #[arg(long, help = generated_help::SESSION_LIST_SELECTOR_HELP)]
    pub selector: Option<String>,
    #[command(flatten)]
    pub scope: NamespaceScopeArgs,
    #[arg(long, help = JSON_OUTPUT_HELP)]
    pub json: bool,
    #[arg(long, help = generated_help::SESSION_LIST_SHOW_LABELS_HELP)]
    pub show_labels: bool,
}

#[derive(Debug, Args)]
pub struct NamespaceGetArgs {
    #[arg(help = generated_help::NAMESPACE_LIST_SLUG_HELP)]
    pub slug: Option<String>,
    #[arg(long, help = JSON_OUTPUT_HELP)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[command(subcommand)]
    pub resource: CreateResource,
}

#[derive(Debug, Subcommand)]
pub enum CreateResource {
    #[command(
        about = "Create a namespace before running sessions into it",
        long_about = "Create a namespace before running sessions into it"
    )]
    Namespace(NamespaceCreateArgs),
    #[command(
        about = "Declaratively create a headless session record",
        long_about = "Declaratively create a headless session record"
    )]
    Session(SessionCreateArgs),
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct NamespaceCreateArgs {
    #[arg(help = NAMESPACE_CREATE_HELP)]
    pub slug: String,
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    #[command(
        about = "Set the user namespace context used by CLI commands",
        long_about = "Set the user namespace context used by CLI commands"
    )]
    SetContext(SetContextArgs),
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct SetContextArgs {
    #[arg(help = NAMESPACE_CONTEXT_HELP)]
    pub namespace: Namespace,
}

#[derive(Debug, Args)]
pub struct DeleteArgs {
    #[command(subcommand)]
    pub resource: DeleteResource,
}

#[derive(Debug, Subcommand)]
pub enum DeleteResource {
    #[command(about = generated_help::SESSION_DELETE_ABOUT, long_about = generated_help::SESSION_DELETE_ABOUT)]
    #[command(alias = "sessions")]
    Session(DeleteSessionArgs),
    #[command(
        about = "Delete a namespace, terminate its sessions, and clear matching user context",
        long_about = "Delete a namespace, terminate its sessions, and clear matching user context"
    )]
    Namespace(DeleteNamespaceArgs),
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct DeleteSessionArgs {
    #[arg(help = generated_help::SESSION_DELETE_SELECTOR_HELP)]
    pub selector: String,
    #[command(flatten)]
    pub scope: NamespaceScopeArgs,
    #[arg(long, default_value = "SIGTERM", help = generated_help::SESSION_DELETE_SIGNAL_HELP)]
    pub signal: String,
    #[arg(long, default_value_t = 5, help = generated_help::SESSION_DELETE_GRACE_SECS_HELP)]
    pub grace: u64,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct DeleteNamespaceArgs {
    #[arg(help = NAMESPACE_DELETE_HELP)]
    pub namespace: Namespace,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct LogsArgs {
    #[arg(help = generated_help::LOGS_SELECTOR_HELP)]
    pub selector: String,
    #[arg(short = 'f', long, help = generated_help::LOGS_FOLLOW_HELP)]
    pub follow: bool,
    #[arg(long = "max-bytes", help = generated_help::LOGS_MAX_BYTES_HELP)]
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct CaptureArgs {
    #[arg(help = generated_help::SESSION_CAPTURE_ID_HELP)]
    pub session_id: uuid::Uuid,
    #[arg(long = "scrollback-lines", help = generated_help::SESSION_CAPTURE_SCROLLBACK_LINES_HELP)]
    pub scrollback_lines: Option<u32>,
    #[arg(long, help = generated_help::SESSION_CAPTURE_JSON_HELP)]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct WaitArgs {
    #[arg(help = generated_help::WAIT_SELECTOR_HELP)]
    pub selector: String,
    #[arg(long = "for", help = generated_help::WAIT_FOR_HELP)]
    pub condition: String,
    #[arg(long, default_value_t = 30, help = generated_help::WAIT_TIMEOUT_SECS_HELP)]
    pub timeout_secs: u64,
}

#[derive(Debug, Args)]
pub struct MailArgs {
    #[command(subcommand)]
    pub action: MailAction,
}

#[derive(Debug, Subcommand)]
pub enum MailAction {
    #[command(about = generated_help::MAIL_SEND_ABOUT, long_about = generated_help::MAIL_SEND_ABOUT)]
    Send(MailSendArgs),
    #[command(about = generated_help::MAIL_READ_ABOUT, long_about = generated_help::MAIL_READ_ABOUT)]
    Read(MailReadArgs),
    #[command(about = generated_help::MAIL_CHECK_ABOUT, long_about = generated_help::MAIL_CHECK_ABOUT)]
    Check(MailCheckArgs),
    #[command(name = "stop-check", about = generated_help::MAIL_STOP_CHECK_ABOUT, long_about = generated_help::MAIL_STOP_CHECK_ABOUT)]
    StopCheck(MailStopCheckArgs),
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct MailSendArgs {
    #[arg(long, help = generated_help::MAIL_SEND_TO_HELP)]
    pub to: String,
    #[command(flatten)]
    pub scope: NamespaceScopeArgs,
    #[arg(long, help = generated_help::MAIL_SEND_FROM_HELP)]
    pub from: Option<String>,
    #[arg(long, help = generated_help::MAIL_SEND_CONTENT_HELP)]
    pub content: String,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct MailReadArgs {
    #[arg(long, alias = "from", help = generated_help::MAIL_READ_SELECTOR_HELP)]
    pub selector: String,
    #[arg(long, help = generated_help::MAIL_READ_PEEK_HELP)]
    pub peek: bool,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct MailCheckArgs {
    #[arg(long, alias = "from", help = generated_help::MAIL_CHECK_SELECTOR_HELP)]
    pub selector: String,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct MailStopCheckArgs {
    #[arg(long, alias = "from", help = generated_help::MAIL_STOP_CHECK_SELECTOR_HELP)]
    pub selector: String,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct LabelArgs {
    #[arg(help = generated_help::SESSION_LABEL_SELECTOR_HELP)]
    pub selector: String,
    #[command(flatten)]
    pub scope: NamespaceScopeArgs,
    #[arg(help = generated_help::SESSION_LABEL_MUTATION_HELP)]
    pub mutation: String,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct NudgeArgs {
    #[arg(long, help = generated_help::NUDGE_TO_HELP)]
    pub to: String,
    #[command(flatten)]
    pub scope: NamespaceScopeArgs,
    #[arg(long, help = generated_help::NUDGE_CONTENT_HELP)]
    pub content: String,
}

#[derive(Debug, Args)]
pub struct McpArgs {}
