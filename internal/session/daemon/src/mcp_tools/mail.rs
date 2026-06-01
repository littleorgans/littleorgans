use anyhow::Result;
use lilo_session_core::{
    MailCheckRequest, MailCountView, MailIntent, MailNotifyMode, MailReadRequest, MailSendRequest,
    MailStopCheckRequest, RpcResponse, Selector, SessionRpc, tool_success,
};
use serde_json::{Value, json};
use std::str::FromStr;

use crate::handler::DaemonState;
use crate::identity_client::RequestContext;

use super::agent::session_tool_response_error;
use super::args::{
    optional_bool, optional_string, required_selector, required_string, scoped_required_selector,
};

pub(crate) async fn mail_send(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let to = required_selector(arguments, "to")?;
    let to = scoped_required_selector(state, context, arguments, to).await?;
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::MailSend {
                request: MailSendRequest {
                    to,
                    content: required_string(arguments, "content")?.to_string(),
                    notify: optional_notify(arguments)?,
                    context_id: required_string(arguments, "context_id")?.to_string(),
                    intent: required_intent(arguments)?,
                    idempotency_key: optional_string(arguments, "idempotency_key")
                        .map(ToString::to_string),
                },
            },
        )
        .await;
    match response.response {
        RpcResponse::MailSent { response } => Ok(tool_success(
            format!("sent {} mail item(s)", response.results.len()),
            &json!({ "results": response.results, "errors": response.errors }),
        )),
        other => Err(session_tool_response_error(&other)),
    }
}

pub(crate) async fn mail_read(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::MailRead {
                request: MailReadRequest {
                    peek: optional_bool(arguments, "peek").unwrap_or(false),
                },
            },
        )
        .await;
    match response.response {
        RpcResponse::MailRead { response } => {
            let count = response.messages.len();
            Ok(tool_success(
                format!("{count} mail item(s)"),
                &json!({ "messages": response.messages, "errors": response.errors }),
            ))
        }
        other => Err(session_tool_response_error(&other)),
    }
}

pub(crate) async fn mail_check(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    mail_count_from_args(state, context, arguments, |selector| {
        SessionRpc::MailCheck {
            request: MailCheckRequest { selector },
        }
    })
    .await
}

pub(crate) async fn mail_stop_check(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    mail_count_from_args(state, context, arguments, |selector| {
        SessionRpc::MailStopCheck {
            request: MailStopCheckRequest { selector },
        }
    })
    .await
}

async fn mail_count_from_args(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
    request: impl FnOnce(Selector) -> SessionRpc,
) -> Result<Value> {
    let selector = required_selector(arguments, "selector")?;
    let selector = scoped_required_selector(state, context, arguments, selector).await?;
    mail_count_tool(state, context, request(selector)).await
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
        other => Err(session_tool_response_error(&other)),
    }
}

fn unread_tool_response(unread: usize, counts: &[MailCountView]) -> Value {
    tool_success(
        format!("{unread} unread"),
        &json!({ "unread": unread, "counts": counts }),
    )
}

fn required_intent(arguments: &Value) -> Result<MailIntent> {
    MailIntent::from_client_send_str(required_string(arguments, "intent")?).map_err(Into::into)
}

fn optional_notify(arguments: &Value) -> Result<Option<MailNotifyMode>> {
    optional_string(arguments, "notify")
        .map(MailNotifyMode::from_str)
        .transpose()
        .map_err(Into::into)
}
