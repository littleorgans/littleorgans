use std::path::Path;
use std::str::FromStr;

use anyhow::{Result, anyhow};
use lilo_session_core::{
    CaptureRequest, DeleteRequest, IsolationPolicy, LabelMutation, LabelRequest, ListRequest,
    RpcResponse, RuntimeKind, Selector, SessionRpc, SpawnRequest, normalize_agent_config_request,
    tool_success,
};
use serde_json::{Value, json};

use crate::handler::DaemonState;
use crate::identity_client::RequestContext;

use super::args::{
    optional_bool, optional_labels, optional_mounts, optional_selector, optional_string,
    optional_u64, required_selector, required_string, scoped_optional_selector,
    scoped_required_selector, selector_from_id, unexpected_response,
};

pub(crate) async fn agent_run(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let runtime = RuntimeKind::from_str(required_string(arguments, "runtime")?)?;
    let role = required_string(arguments, "role")?.to_string();
    let dir = required_string(arguments, "dir")?.to_string();
    let namespace = optional_string(arguments, "namespace")
        .map(lilo_session_core::Namespace::from_str)
        .transpose()?;
    let labels = optional_labels(arguments)?;
    let agent_config = optional_string(arguments, "agent_config")
        .map(|value| normalize_agent_config_request(value, Path::new(&dir), None));
    let target = optional_string(arguments, "target")
        .unwrap_or("headless")
        .to_string();
    let force = optional_bool(arguments, "force").unwrap_or(false);
    let isolation = optional_string(arguments, "isolation")
        .map(IsolationPolicy::from_str)
        .transpose()
        .map_err(|error| anyhow!(error))?
        .unwrap_or_default();
    let image = optional_string(arguments, "image").map(str::to_string);
    let mounts = optional_mounts(arguments)?;
    if isolation.is_host() && !mounts.is_empty() {
        anyhow::bail!("--mount is docker-only and cannot be used with --isolation host");
    }
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime,
                    role,
                    workspace: dir.clone(),
                    dir: Some(dir),
                    namespace,
                    target,
                    agent_config,
                    isolation,
                    image,
                    env: Vec::new(),
                    mounts,
                    shell_resume: None,
                    labels,
                    force,
                }),
            },
        )
        .await;
    match response.response {
        RpcResponse::Spawned { response } => {
            let text = format!("spawned {}", response.session.id);
            Ok(tool_success(text, &json!({ "session": response.session })))
        }
        RpcResponse::Error { message } => Err(anyhow!(message)),
        other => Err(unexpected_response(&other)),
    }
}

pub(crate) async fn agent_list(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let selector = optional_selector(arguments, "selector")?
        .or_else(|| optional_string(arguments, "id").and_then(|id| selector_from_id(id).ok()));
    let selector = scoped_optional_selector(state, context, arguments, selector).await?;
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::List {
                request: ListRequest { selector },
            },
        )
        .await;
    match response.response {
        RpcResponse::Listed { response } => {
            let count = response.sessions.len();
            Ok(tool_success(
                format!("{count} session(s)"),
                &json!({ "sessions": response.sessions }),
            ))
        }
        RpcResponse::Error { message } => Err(anyhow!(message)),
        other => Err(unexpected_response(&other)),
    }
}

pub(crate) async fn agent_get(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let id = required_string(arguments, "id")?.to_string();
    let selector = Selector::Id {
        id: uuid::Uuid::parse_str(&id)?,
    };
    let selector = scoped_required_selector(state, context, arguments, selector).await?;
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::List {
                request: ListRequest {
                    selector: Some(selector),
                },
            },
        )
        .await;
    match response.response {
        RpcResponse::Listed { response } => {
            let session = response
                .sessions
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("unknown session: {id}"))?;
            Ok(tool_success(
                format!("found {}", session.id),
                &json!({ "session": session }),
            ))
        }
        RpcResponse::Error { message } => Err(anyhow!(message)),
        other => Err(unexpected_response(&other)),
    }
}

pub(crate) async fn agent_capture(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let selector = selector_from_id(required_string(arguments, "id")?)?;
    let selector = scoped_required_selector(state, context, arguments, selector).await?;
    let session_id = state
        .resolve_selector(&selector, "capture")
        .await?
        .pop()
        .ok_or_else(|| anyhow!("capture selector matched no sessions: {selector}"))?
        .id;
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::Capture {
                request: CaptureRequest {
                    session_id,
                    scrollback_lines: optional_u64(arguments, "scrollback_lines")
                        .map(u32::try_from)
                        .transpose()
                        .map_err(|_| anyhow!("scrollback_lines is out of range"))?,
                },
            },
        )
        .await;
    match response.response {
        RpcResponse::Capture { response } => Ok(tool_success(
            format!("captured {}", response.session.id),
            &json!({ "session": response.session, "capture": response.capture }),
        )),
        RpcResponse::Error { message } => Err(anyhow!(message)),
        other => Err(unexpected_response(&other)),
    }
}

pub(crate) async fn agent_delete(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let selector = required_selector(arguments, "selector")
        .or_else(|_| required_string(arguments, "id").and_then(selector_from_id))?;
    let signal = optional_string(arguments, "signal")
        .unwrap_or("SIGTERM")
        .to_string();
    let grace_secs = optional_u64(arguments, "grace_secs").unwrap_or(5);
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::Delete {
                request: DeleteRequest {
                    selector,
                    signal,
                    grace_secs,
                },
            },
        )
        .await;
    match response.response {
        RpcResponse::Deleted { response } => {
            let text = format!("deleted {} session(s)", response.sessions.len());
            Ok(tool_success(
                text,
                &json!({ "sessions": response.sessions, "errors": response.errors }),
            ))
        }
        RpcResponse::Error { message } => Err(anyhow!(message)),
        other => Err(unexpected_response(&other)),
    }
}

pub(crate) async fn agent_label(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let selector = required_selector(arguments, "selector")?;
    let mutation = LabelMutation::from_str(required_string(arguments, "mutation")?)?;
    let response = state
        .handle_direct(
            context.clone(),
            SessionRpc::Label {
                request: LabelRequest { selector, mutation },
            },
        )
        .await;
    match response.response {
        RpcResponse::Labeled { response } => Ok(tool_success(
            format!("labeled {} session(s)", response.sessions.len()),
            &json!({ "sessions": response.sessions, "errors": response.errors }),
        )),
        RpcResponse::Error { message } => Err(anyhow!(message)),
        other => Err(unexpected_response(&other)),
    }
}
