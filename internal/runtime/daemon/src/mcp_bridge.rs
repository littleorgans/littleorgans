use std::sync::Arc;

use anyhow::{Result, anyhow};
use lilo_rm_core::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, KillByPidRequest, MCP_PROTOCOL_VERSION,
    StatusFilter, StatusResponse, json_rpc_error, json_rpc_failure, json_rpc_result,
    tool_contracts::contract_registry, tool_error, tool_success,
};
use serde_json::{Value, json};

use crate::server::ServerState;

pub(crate) async fn handle_line(state: &Arc<ServerState>, line: &str) -> Option<String> {
    let response = match serde_json::from_str::<JsonRpcRequest>(line) {
        Ok(request) => handle_request(state, request).await?,
        Err(error) => json_rpc_failure(
            Value::Null,
            json_rpc_error(-32700, format!("Parse error: {error}")),
        ),
    };
    Some(serde_json::to_string(&response).unwrap_or_else(|error| {
        json!({
            "jsonrpc": "2.0",
            "id": Value::Null,
            "error": {
                "code": -32603,
                "message": format!("Internal error: {error}"),
            },
        })
        .to_string()
    }))
}

async fn handle_request(
    state: &Arc<ServerState>,
    request: JsonRpcRequest,
) -> Option<JsonRpcResponse> {
    let id = request.id.unwrap_or(Value::Null);
    if request.method.starts_with("notifications/") {
        return None;
    }

    let result = match request.method.as_str() {
        "initialize" => Ok(initialize_result()),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(contract_registry().tool_list_value()),
        "tools/call" => handle_tool_call(state, request.params).await,
        other => Err(json_rpc_error(-32601, format!("Method not found: {other}"))),
    };

    Some(match result {
        Ok(result) => json_rpc_result(id, result),
        Err(error) => json_rpc_failure(id, error),
    })
}

fn initialize_result() -> Value {
    let version = crate::version::runtime_version_info();
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "rtm",
            "version": version.version
        },
        "instructions": "runtime-matters admin MCP exposes rtmd substrate operations only."
    })
}

async fn handle_tool_call(
    state: &Arc<ServerState>,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or_else(|| json_rpc_error(-32602, "Missing params"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| json_rpc_error(-32602, "Missing tool name"))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    Ok(match call_tool(state, name, arguments).await {
        Ok(value) => value,
        Err(error) => tool_error(error.to_string()),
    })
}

async fn call_tool(state: &Arc<ServerState>, name: &str, arguments: Value) -> Result<Value> {
    match name {
        "rtm_kill_by_pid" => kill_by_pid(state, arguments).await,
        "rtm_status" => status(state, arguments).await,
        "rtm_version" => version(&arguments),
        "rtm_watchers" => watchers(state, arguments).await,
        other => Ok(tool_error(format!("Unknown tool: {other}"))),
    }
}

async fn kill_by_pid(state: &Arc<ServerState>, arguments: Value) -> Result<Value> {
    let request: KillByPidRequest = serde_json::from_value(arguments)?;
    let response = state.kill_pid(request).await?;
    let text = serde_json::to_string(&response)?;
    Ok(tool_success(text, &response))
}

async fn status(state: &Arc<ServerState>, arguments: Value) -> Result<Value> {
    let filter: StatusFilter = serde_json::from_value(arguments)?;
    let response = StatusResponse {
        lifecycles: state.status(filter).await,
    };
    let text = serde_json::to_string(&response.lifecycles)?;
    Ok(tool_success(text, &response))
}

fn version(arguments: &Value) -> Result<Value> {
    ensure_empty_arguments(arguments)?;
    let response = crate::version::runtime_version_info();
    let text = serde_json::to_string(&response)?;
    Ok(tool_success(text, &response))
}

async fn watchers(state: &Arc<ServerState>, arguments: Value) -> Result<Value> {
    ensure_empty_arguments(&arguments)?;
    let response = state.watcher_counts().await;
    let text = serde_json::to_string(&response)?;
    Ok(tool_success(text, &response))
}

fn ensure_empty_arguments(arguments: &Value) -> Result<()> {
    if arguments.as_object().is_some_and(serde_json::Map::is_empty) {
        return Ok(());
    }
    Err(anyhow!("tool does not accept arguments"))
}
