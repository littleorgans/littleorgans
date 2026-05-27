mod agent;
mod args;
mod control;
mod mail;
mod namespace;

use anyhow::Result;
use lilo_session_core::tool_error;
use serde_json::Value;

use crate::handler::DaemonState;
use crate::identity_client::RequestContext;

pub(crate) use agent::{
    agent_capture, agent_delete, agent_get, agent_label, agent_list, agent_run,
};
pub(crate) use control::{doctor, logs, nudge, wait};
pub(crate) use mail::{mail_check, mail_read, mail_send, mail_stop_check};
pub(crate) use namespace::{namespace_get, namespace_list};

pub async fn call_tool(
    state: &DaemonState,
    context: &RequestContext,
    name: &str,
    arguments: &Value,
) -> Result<Value> {
    match name {
        "agent_run" | "session_run" => agent_run(state, context, arguments).await,
        "agent_list" | "session_list" => agent_list(state, context, arguments).await,
        "agent_get" | "session_get" => agent_get(state, context, arguments).await,
        "namespace_list" => namespace_list(state, context, arguments).await,
        "namespace_get" => namespace_get(state, context, arguments).await,
        "agent_capture" | "session_capture" => agent_capture(state, context, arguments).await,
        "agent_delete" | "session_delete" => agent_delete(state, context, arguments).await,
        "agent_label" | "session_label" => agent_label(state, context, arguments).await,
        "mail_send" => mail_send(state, context, arguments).await,
        "mail_read" => mail_read(state, context, arguments).await,
        "mail_check" => mail_check(state, context, arguments).await,
        "mail_stop_check" => mail_stop_check(state, context, arguments).await,
        "nudge" => nudge(state, context, arguments).await,
        "logs" => logs(state, context, arguments).await,
        "wait" => wait(state, context, arguments).await,
        "doctor" => doctor(state, context, arguments).await,
        other => Ok(tool_error(format!("Unknown tool: {other}"))),
    }
}
