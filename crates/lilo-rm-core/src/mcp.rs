use crate::isolation::IsolationPolicy;
use crate::types::MountSpec;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use thiserror::Error;

pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct McpBridgeRequest {
    pub line: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct McpBridgeResponse {
    pub line: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum McpRequest {
    Initialize,
    Ping,
    ToolsList,
    ToolsCall(Option<Value>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallRequest {
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("--mount is docker-only and cannot be used with --isolation host")]
pub struct HostMountPolicyError;

pub fn parse_json_rpc_line(line: &str) -> Result<JsonRpcRequest, Box<JsonRpcResponse>> {
    serde_json::from_str::<JsonRpcRequest>(line).map_err(|error| {
        Box::new(json_rpc_failure(
            Value::Null,
            json_rpc_error(-32700, format!("Parse error: {error}")),
        ))
    })
}

pub fn serialize_json_rpc_response(response: &JsonRpcResponse) -> String {
    serde_json::to_string(response).unwrap_or_else(|error| {
        json!({
            "jsonrpc": "2.0",
            "id": response.id.clone(),
            "error": json_rpc_error(-32603, format!("failed to serialize response: {error}"))
        })
        .to_string()
    })
}

pub fn prepare_mcp_request(
    request: JsonRpcRequest,
) -> Option<(Value, Result<McpRequest, JsonRpcError>)> {
    let id = request.id.unwrap_or(Value::Null);
    if request.method.starts_with("notifications/") {
        return None;
    }

    let request = match request.method.as_str() {
        "initialize" => Ok(McpRequest::Initialize),
        "ping" => Ok(McpRequest::Ping),
        "tools/list" => Ok(McpRequest::ToolsList),
        "tools/call" => Ok(McpRequest::ToolsCall(request.params)),
        other => Err(json_rpc_error(-32601, format!("Method not found: {other}"))),
    };

    Some((id, request))
}

pub fn json_rpc_response_from_result(
    id: Value,
    result: Result<Value, JsonRpcError>,
) -> JsonRpcResponse {
    match result {
        Ok(result) => json_rpc_result(id, result),
        Err(error) => json_rpc_failure(id, error),
    }
}

pub fn json_rpc_result(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_owned(),
        id,
        result: Some(result),
        error: None,
    }
}

pub fn json_rpc_failure(id: Value, error: JsonRpcError) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_owned(),
        id,
        result: None,
        error: Some(error),
    }
}

pub fn json_rpc_error(code: i32, message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code,
        message: message.into(),
        data: None,
    }
}

pub fn tool_call_request(params: Option<Value>) -> Result<ToolCallRequest, JsonRpcError> {
    let params = params.ok_or_else(|| json_rpc_error(-32602, "Missing params"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| json_rpc_error(-32602, "Missing tool name"))?
        .to_owned();
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(ToolCallRequest { name, arguments })
}

pub fn tool_success<T>(text: impl Into<String>, structured: &T) -> Value
where
    T: Serialize,
{
    json!({
        "content": [{"type": "text", "text": text.into()}],
        "structuredContent": serde_json::to_value(structured)
            .expect("structured MCP result serializes")
    })
}

pub fn tool_error(message: impl Into<String>) -> Value {
    tool_error_with_meta_key("rtm_tool_error", message)
}

pub fn tool_error_with_meta_key(meta_key: impl Into<String>, message: impl Into<String>) -> Value {
    let message = message.into();
    let mut meta = Map::new();
    meta.insert(
        meta_key.into(),
        json!({
            "is_error": true,
            "message": message
        }),
    );
    json!({
        "content": [{"type": "text", "text": format!("ERROR: {message}")}],
        "_meta": Value::Object(meta)
    })
}

pub fn ensure_mounts_allowed_for_isolation(
    isolation: &IsolationPolicy,
    mounts: &[MountSpec],
) -> Result<(), HostMountPolicyError> {
    if isolation.is_host() && !mounts.is_empty() {
        return Err(HostMountPolicyError);
    }
    Ok(())
}
