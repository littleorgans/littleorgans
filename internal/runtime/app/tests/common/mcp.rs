use serde_json::{Value, json};

use super::{RtmHarness, output_stdout};

pub fn call_tool(
    harness: &RtmHarness,
    id: u32,
    name: &str,
    arguments: impl serde::Serialize,
) -> Value {
    mcp_json(
        harness,
        request(
            id,
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments
            }),
        ),
    )
}

pub fn request(id: u32, method: &str, params: impl serde::Serialize) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
    .to_string()
}

pub fn mcp_json(harness: &RtmHarness, request: impl AsRef<str>) -> Value {
    let output = harness.mcp_line(request.as_ref());
    let success = output.status.success();
    let stdout = output_stdout(output);
    assert!(success, "mcp failed: stdout={stdout}");
    serde_json::from_str(stdout.trim()).expect("mcp json")
}

pub fn tool_names(response: &Value) -> Vec<&str> {
    response["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect()
}

pub fn content_text(response: &Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .expect("content text")
}
