use serde_json::Value;

pub use lilo_rm_core::mcp::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION, McpRequest,
    ToolCallRequest, json_rpc_error, json_rpc_failure, json_rpc_response_from_result,
    json_rpc_result, parse_json_rpc_line, prepare_mcp_request, serialize_json_rpc_response,
    tool_call_request, tool_error_with_meta_key, tool_success,
};

const SESSION_TOOL_ERROR_META_KEY: &str = "sm_tool_error";

pub fn tool_error(message: impl Into<String>) -> Value {
    tool_error_with_meta_key(SESSION_TOOL_ERROR_META_KEY, message)
}
