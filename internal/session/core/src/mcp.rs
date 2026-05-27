use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

pub fn tool_success<T>(text: String, structured: &T) -> Value
where
    T: Serialize,
{
    let structured_content = match serde_json::to_value(structured) {
        Ok(value) => value,
        Err(error) => panic!("structured MCP result serializes: {error}"),
    };

    let mut content_item = Map::new();
    content_item.insert("type".to_string(), Value::String("text".to_string()));
    content_item.insert("text".to_string(), Value::String(text));

    let mut result = Map::new();
    result.insert(
        "content".to_string(),
        Value::Array(vec![Value::Object(content_item)]),
    );
    result.insert("structuredContent".to_string(), structured_content);
    Value::Object(result)
}

pub fn tool_error(message: impl Into<String>) -> Value {
    let message = message.into();
    json!({
        "content": [{"type": "text", "text": format!("ERROR: {message}")}],
        "_meta": {
            "sm_tool_error": {
                "is_error": true,
                "message": message
            }
        }
    })
}
