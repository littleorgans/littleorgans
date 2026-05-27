use anyhow::Result;
use lilo_session_core::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION,
    tool_contracts::contract_registry, tool_error,
};
use serde_json::{Value, json};

use crate::handler::DaemonState;
use crate::identity_client::RequestContext;

pub async fn handle_line(
    state: &DaemonState,
    context: &RequestContext,
    line: &str,
) -> Option<String> {
    let response = match serde_json::from_str::<JsonRpcRequest>(line) {
        Ok(request) => handle_request(state, context, request).await?,
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Value::Null,
            result: None,
            error: Some(json_rpc_error(-32700, format!("Parse error: {error}"))),
        },
    };
    Some(serialize_response(&response))
}

fn serialize_response(response: &JsonRpcResponse) -> String {
    match serde_json::to_string(response) {
        Ok(line) => line,
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": response.id.clone(),
            "error": json_rpc_error(-32603, format!("failed to serialize response: {error}"))
        })
        .to_string(),
    }
}

async fn handle_request(
    state: &DaemonState,
    context: &RequestContext,
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
        "tools/call" => handle_tool_call(state, context, request.params).await,
        other => Err(json_rpc_error(-32601, format!("Method not found: {other}"))),
    };

    Some(match result {
        Ok(result) => json_rpc_result(id, result),
        Err(error) => json_rpc_failure(id, error),
    })
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "sm",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": server_instructions()
    })
}

async fn handle_tool_call(
    state: &DaemonState,
    context: &RequestContext,
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

    Ok(
        match crate::mcp_tools::call_tool(state, context, name, &arguments).await {
            Ok(value) => value,
            Err(error) => tool_error(error.to_string()),
        },
    )
}

fn json_rpc_result(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(result),
        error: None,
    }
}

fn json_rpc_failure(id: Value, error: JsonRpcError) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(error),
    }
}

fn json_rpc_error(code: i32, message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code,
        message: message.into(),
        data: None,
    }
}

fn server_instructions() -> String {
    let overview = contract_registry()
        .tools()
        .iter()
        .map(|tool| format!("- {}: {}", tool.name, tool.mcp_description))
        .collect::<Vec<_>>()
        .join("\n");
    format!("session-matters controls local Helioy agent sessions.\n\n{overview}")
}
