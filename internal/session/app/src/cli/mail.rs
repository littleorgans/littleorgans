use anyhow::{Result, bail};
use std::str::FromStr;

use lilo_session_core::{
    MailCheckRequest, MailReadRequest, MailSendRequest, MailStopCheckRequest, RpcRequest,
    RpcResponse, Selector, SmEndpoint,
};

use crate::cli::cli_def::{
    MailAction, MailArgs, MailCheckArgs, MailReadArgs, MailSendArgs, MailStopCheckArgs,
};
use crate::cli::output::print_mail;
use crate::cli::selector_scope::required_scoped_selector;

pub async fn run(args: MailArgs) -> Result<()> {
    match args.action {
        MailAction::Send(args) => send(args).await,
        MailAction::Read(args) => read(args).await,
        MailAction::Check(args) => check(args).await,
        MailAction::StopCheck(args) => stop_check(args).await,
    }
}

async fn send(args: MailSendArgs) -> Result<()> {
    let response = send_daemon_request(RpcRequest::MailSend {
        request: MailSendRequest {
            from: args.from.or_else(env_session_id),
            to: required_scoped_selector(&args.to, &args.scope)?,
            content: args.content,
        },
    })
    .await?;

    match response {
        RpcResponse::MailSent { response } => {
            for item in response.mail {
                println!("{}", item.id);
            }
            for error in response.errors {
                eprintln!("{} {}", error.target, error.message);
            }
            Ok(())
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

async fn read(args: MailReadArgs) -> Result<()> {
    let response = send_daemon_request(RpcRequest::MailRead {
        request: MailReadRequest {
            selector: Selector::from_str(&args.selector)?,
            peek: args.peek,
        },
    })
    .await?;

    match response {
        RpcResponse::MailRead { response } => {
            print_mail(&response.mail);
            for error in response.errors {
                eprintln!("{} {}", error.target, error.message);
            }
            Ok(())
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

async fn check(args: MailCheckArgs) -> Result<()> {
    let unread = unread_count(args.selector).await?;
    println!("{unread} unread");
    Ok(())
}

async fn stop_check(args: MailStopCheckArgs) -> Result<()> {
    let response = send_daemon_request(RpcRequest::MailStopCheck {
        request: MailStopCheckRequest {
            selector: Selector::from_str(&args.selector)?,
        },
    })
    .await?;
    let unread = match response {
        RpcResponse::MailStopChecked { response } => response.unread,
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    };
    if unread == 0 {
        return Ok(());
    }

    println!(
        "{}",
        serde_json::json!({
            "decision": "block",
            "reason": format!("{unread} unread message(s). Run `sm mail read` to drain mail."),
        })
    );
    std::process::exit(2);
}

async fn unread_count(selector: String) -> Result<usize> {
    let response = send_daemon_request(RpcRequest::MailCheck {
        request: MailCheckRequest {
            selector: Selector::from_str(&selector)?,
        },
    })
    .await?;

    match response {
        RpcResponse::MailChecked { response } => Ok(response.unread),
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

async fn send_daemon_request(request: RpcRequest) -> Result<RpcResponse> {
    let endpoint = SmEndpoint::from_env()?;
    lilo_session_daemon::send_request(&endpoint, &request).await
}

fn env_session_id() -> Option<String> {
    std::env::var("HELIOY_SESSION_ID").ok()
}
