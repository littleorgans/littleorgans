use anyhow::Result;
use lilo_session_core::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION,
    mcp::{
        McpRequest, json_rpc_response_from_result, parse_json_rpc_line, prepare_mcp_request,
        serialize_json_rpc_response, tool_call_request,
    },
    tool_contracts::contract_registry,
    tool_error,
};
use serde_json::{Value, json};

use crate::handler::DaemonState;
use crate::identity_client::RequestContext;

pub async fn handle_line(
    state: &DaemonState,
    context: &RequestContext,
    line: &str,
) -> Option<String> {
    let response = match parse_json_rpc_line(line) {
        Ok(request) => handle_request(state, context, request).await?,
        Err(response) => *response,
    };
    Some(serialize_json_rpc_response(&response))
}

async fn handle_request(
    state: &DaemonState,
    context: &RequestContext,
    request: JsonRpcRequest,
) -> Option<JsonRpcResponse> {
    let (id, request) = prepare_mcp_request(request)?;
    let result = match request {
        Ok(McpRequest::Initialize) => Ok(initialize_result()),
        Ok(McpRequest::Ping) => Ok(json!({})),
        Ok(McpRequest::ToolsList) => Ok(contract_registry().tool_list_value()),
        Ok(McpRequest::ToolsCall(params)) => handle_tool_call(state, context, params).await,
        Err(error) => Err(error),
    };
    Some(json_rpc_response_from_result(id, result))
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
    let tool_call = tool_call_request(params)?;

    Ok(
        match crate::mcp_tools::call_tool(state, context, &tool_call.name, &tool_call.arguments)
            .await
        {
            Ok(value) => value,
            Err(error) => tool_error(error.to_string()),
        },
    )
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
