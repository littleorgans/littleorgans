use std::sync::Arc;

use anyhow::{Result, anyhow};
use lilo_rm_core::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, KillByPidRequest, MCP_PROTOCOL_VERSION,
    McpRequest, StatusFilter, StatusResponse, json_rpc_response_from_result, parse_json_rpc_line,
    prepare_mcp_request, serialize_json_rpc_response, tool_call_request,
    tool_contracts::contract_registry, tool_error, tool_success,
};
use serde_json::{Value, json};

use crate::server::ServerState;

pub(crate) async fn handle_line(state: &Arc<ServerState>, line: &str) -> Option<String> {
    let response = match parse_json_rpc_line(line) {
        Ok(request) => handle_request(state, request).await?,
        Err(response) => *response,
    };
    Some(serialize_json_rpc_response(&response))
}

async fn handle_request(
    state: &Arc<ServerState>,
    request: JsonRpcRequest,
) -> Option<JsonRpcResponse> {
    let (id, request) = prepare_mcp_request(request)?;
    let result = match request {
        Ok(McpRequest::Initialize) => Ok(initialize_result()),
        Ok(McpRequest::Ping) => Ok(json!({})),
        Ok(McpRequest::ToolsList) => Ok(contract_registry().tool_list_value()),
        Ok(McpRequest::ToolsCall(params)) => handle_tool_call(state, params).await,
        Err(error) => Err(error),
    };
    Some(json_rpc_response_from_result(id, result))
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
    let tool_call = tool_call_request(params)?;

    Ok(
        match call_tool(state, &tool_call.name, tool_call.arguments).await {
            Ok(value) => value,
            Err(error) => tool_error(error.to_string()),
        },
    )
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
