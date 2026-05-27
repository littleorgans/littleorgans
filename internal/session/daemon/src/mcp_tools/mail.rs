use anyhow::{Result, anyhow};
use lilo_session_core::{
    MailCheckRequest, MailReadRequest, MailSendRequest, MailStopCheckRequest, RpcResponse,
    SessionRpc, tool_success,
};
use serde_json::{Value, json};

use crate::handler::DaemonState;
use crate::identity_client::RequestContext;

use super::args::{
    optional_bool, optional_string, required_selector, required_string, scoped_required_selector,
    unexpected_response,
};

pub(crate) async fn mail_send(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::MailSend {
                request: MailSendRequest {
                    from: optional_string(arguments, "from").map(ToString::to_string),
                    to: required_selector(arguments, "to")?,
                    content: required_string(arguments, "content")?.to_string(),
                },
            },
        )
        .await;
    match response.response {
        RpcResponse::MailSent { response } => Ok(tool_success(
            format!("sent {} mail item(s)", response.mail.len()),
            &json!({ "mail": response.mail, "errors": response.errors }),
        )),
        RpcResponse::Error { message } => Err(anyhow!(message)),
        other => Err(unexpected_response(&other)),
    }
}

pub(crate) async fn mail_read(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let selector = required_selector(arguments, "selector")
        .or_else(|_| required_selector(arguments, "from"))?;
    let selector = scoped_required_selector(state, context, arguments, selector).await?;
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::MailRead {
                request: MailReadRequest {
                    selector,
                    peek: optional_bool(arguments, "peek").unwrap_or(false),
                },
            },
        )
        .await;
    match response.response {
        RpcResponse::MailRead { response } => {
            let count = response.mail.len();
            Ok(tool_success(
                format!("{count} mail item(s)"),
                &json!({ "mail": response.mail, "errors": response.errors }),
            ))
        }
        RpcResponse::Error { message } => Err(anyhow!(message)),
        other => Err(unexpected_response(&other)),
    }
}

pub(crate) async fn mail_check(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let selector = required_selector(arguments, "selector")
        .or_else(|_| required_selector(arguments, "from"))?;
    let selector = scoped_required_selector(state, context, arguments, selector).await?;
    mail_count_tool(
        state,
        context,
        SessionRpc::MailCheck {
            request: MailCheckRequest { selector },
        },
    )
    .await
}

pub(crate) async fn mail_stop_check(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let selector = required_selector(arguments, "selector")
        .or_else(|_| required_selector(arguments, "from"))?;
    let selector = scoped_required_selector(state, context, arguments, selector).await?;
    mail_count_tool(
        state,
        context,
        SessionRpc::MailStopCheck {
            request: MailStopCheckRequest { selector },
        },
    )
    .await
}

async fn mail_count_tool(
    state: &DaemonState,
    context: &RequestContext,
    request: SessionRpc,
) -> Result<Value> {
    match state.handle_direct(context.clone(), request).await.response {
        RpcResponse::MailChecked { response } => {
            Ok(unread_tool_response(response.unread, &response.counts))
        }
        RpcResponse::MailStopChecked { response } => {
            Ok(unread_tool_response(response.unread, &response.counts))
        }
        RpcResponse::Error { message } => Err(anyhow!(message)),
        other => Err(unexpected_response(&other)),
    }
}

fn unread_tool_response(unread: usize, counts: &[lilo_session_core::MailUnreadCount]) -> Value {
    tool_success(
        format!("{unread} unread"),
        &json!({ "unread": unread, "counts": counts }),
    )
}
