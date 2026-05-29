use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use lilo_im_core::{Action, AuditDecision, AuditRow, Principal};
use lilo_session_core::{IdentityAuditRequest, IdentityWhoamiRequest, RpcResponse, SessionRpc};

use super::Output;

#[derive(Debug, Args)]
pub struct IdentityCommand {
    #[command(subcommand)]
    action: IdentityAction,
}

#[derive(Debug, Subcommand)]
enum IdentityAction {
    Whoami(WhoamiArgs),
    Audit(AuditArgs),
}

#[derive(Debug, Args)]
struct WhoamiArgs {}

#[derive(Debug, Args)]
struct AuditArgs {
    #[arg(long)]
    limit: Option<usize>,
}

impl IdentityCommand {
    pub async fn run(&self, output: Output) -> Result<()> {
        match &self.action {
            IdentityAction::Whoami(_) => whoami(output).await,
            IdentityAction::Audit(args) => audit(output, args).await,
        }
    }
}

async fn whoami(output: Output) -> Result<()> {
    let response = send(&SessionRpc::IdentityWhoami {
        request: IdentityWhoamiRequest::default(),
    })
    .await?;

    match response {
        RpcResponse::IdentityWhoami { response } if output == Output::Json => {
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        RpcResponse::IdentityWhoami { response } => {
            println!("{}", principal_label(&response.principal));
            Ok(())
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

async fn audit(output: Output, args: &AuditArgs) -> Result<()> {
    let response = send(&SessionRpc::IdentityAudit {
        request: IdentityAuditRequest {
            limit: args.limit,
            ..IdentityAuditRequest::default()
        },
    })
    .await?;

    match response {
        RpcResponse::IdentityAudit { response } if output == Output::Json => {
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        RpcResponse::IdentityAudit { response } => {
            print_audit_rows(&response.rows);
            Ok(())
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

async fn send(request: &SessionRpc) -> Result<RpcResponse> {
    lilo_session_app::cli::client::send_request(request).await
}

fn print_audit_rows(rows: &[AuditRow]) {
    println!("TIMESTAMP PRINCIPAL ACTION DECISION SESSION");
    for row in rows {
        println!(
            "{} {} {} {} {}",
            row.timestamp.to_rfc3339(),
            principal_label(&row.principal),
            action_label(row.action),
            decision_label(&row.decision),
            row.session_ref
                .map_or_else(|| "-".to_string(), |id| id.to_string())
        );
    }
}

fn principal_label(principal: &Principal) -> String {
    match principal {
        Principal::Local(uid) => format!("local:{uid}"),
        Principal::Unknown { kind, .. } => format!("unknown:{kind}"),
    }
}

fn action_label(action: Action) -> &'static str {
    match action {
        Action::Spawn => "spawn",
        Action::Kill => "kill",
        Action::List => "list",
        Action::Read => "read",
        Action::Logs => "logs",
        Action::MailSend => "mail_send",
        Action::MailRead => "mail_read",
        Action::Nudge => "nudge",
        Action::Link => "link",
        Action::Doctor => "doctor",
        Action::Daemon => "daemon",
        Action::ShimCallback => "shim_callback",
    }
}

fn decision_label(decision: &AuditDecision) -> String {
    match decision {
        AuditDecision::Allow => "allow".to_string(),
        AuditDecision::Deny { reason } => format!("deny:{reason}"),
        AuditDecision::Error { message } => format!("error:{message}"),
    }
}
