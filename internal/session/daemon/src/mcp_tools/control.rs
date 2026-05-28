use std::str::FromStr;

use anyhow::Result;
use lilo_session_core::{
    DoctorRequest, LogsRequest, NudgeRequest, RpcResponse, SessionRpc, WaitCondition, WaitRequest,
    tool_success,
};
use serde_json::{Value, json};

use crate::handler::DaemonState;
use crate::identity_client::RequestContext;

use super::agent::session_tool_response_error;
use super::args::{
    optional_u64, required_selector, required_string, scoped_required_selector, selector_from_id,
};

pub(crate) async fn nudge(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let to = scoped_required_selector(
        state,
        context,
        arguments,
        required_selector(arguments, "to")?,
    )
    .await?;
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::Nudge {
                request: NudgeRequest {
                    to,
                    content: required_string(arguments, "content")?.to_string(),
                },
            },
        )
        .await;
    match response.response {
        RpcResponse::Nudged { response } => Ok(tool_success(
            format!("nudged {} session(s)", response.nudges.len()),
            &json!({
                "nudges": response.nudges,
                "errors": response.errors
            }),
        )),
        other => Err(session_tool_response_error(&other)),
    }
}

pub(crate) async fn logs(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let selector = required_selector(arguments, "selector")
        .or_else(|_| required_string(arguments, "id").and_then(selector_from_id))?;
    let selector = scoped_required_selector(state, context, arguments, selector).await?;
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::Logs {
                request: LogsRequest {
                    selector,
                    max_bytes: optional_u64(arguments, "max_bytes"),
                },
            },
        )
        .await;
    match response.response {
        RpcResponse::Logs { response } => Ok(tool_success(
            format!("logs {}", response.session.id),
            &json!({
                "session": response.session,
                "transcript_path": response.transcript_path,
                "content": response.content
            }),
        )),
        other => Err(session_tool_response_error(&other)),
    }
}

pub(crate) async fn wait(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let condition = WaitCondition::from_str(required_string(arguments, "for")?)?;
    let selector = scoped_required_selector(
        state,
        context,
        arguments,
        required_selector(arguments, "selector")?,
    )
    .await?;
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::Wait {
                request: WaitRequest {
                    selector,
                    condition,
                    timeout_secs: optional_u64(arguments, "timeout_secs").unwrap_or(30),
                },
            },
        )
        .await;
    match response.response {
        RpcResponse::Wait { response } => Ok(tool_success(
            format!("wait matched: {}", response.matched),
            &json!({ "matched": response.matched, "sessions": response.sessions }),
        )),
        other => Err(session_tool_response_error(&other)),
    }
}

pub(crate) async fn doctor(
    state: &DaemonState,
    context: &RequestContext,
    _arguments: &Value,
) -> Result<Value> {
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::Doctor {
                request: DoctorRequest::default(),
            },
        )
        .await;
    match response.response {
        RpcResponse::Doctor { response } => Ok(tool_success(
            format!("doctor {}", response.status),
            &json!({
                "status": response.status,
                "runtime": response.runtime,
                "runtime_matters": response.runtime_matters,
                "findings": response.findings
            }),
        )),
        other => Err(session_tool_response_error(&other)),
    }
}
