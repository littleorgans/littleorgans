use crate::common::{self, DaemonFixture, OrPanic as _};
use lilo_im_core::{Action, AuditRow};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Stdio;

pub(crate) fn call_tool(
    mcp: &mut common::McpFixture,
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    let mut params = serde_json::Map::new();
    params.insert("name".to_string(), Value::String(name.to_string()));
    params.insert("arguments".to_string(), arguments);
    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": params
    }))
}

pub(crate) fn spawn_agent(
    mcp: &mut common::McpFixture,
    id: u64,
    role: &str,
    workspace: &Path,
) -> String {
    spawn_agent_with_labels(mcp, id, role, workspace, json!([]))
}

pub(crate) fn spawn_agent_in_namespace(
    mcp: &mut common::McpFixture,
    id: u64,
    role: &str,
    workspace: &Path,
    namespace: &str,
) -> String {
    let spawned = call_tool(
        mcp,
        id,
        "session_run",
        json!({
            "runtime": "codex",
            "role": role,
            "dir": workspace.display().to_string(),
            "namespace": namespace
        }),
    );
    assert!(spawned["error"].is_null());
    spawned["result"]["structuredContent"]["session"]["id"]
        .as_str()
        .or_panic("spawn returns session id")
        .to_string()
}

pub(crate) fn spawn_agent_with_labels(
    mcp: &mut common::McpFixture,
    id: u64,
    role: &str,
    workspace: &Path,
    labels: Value,
) -> String {
    let mut arguments = serde_json::Map::new();
    arguments.insert("runtime".to_string(), Value::String("codex".to_string()));
    arguments.insert("role".to_string(), Value::String(role.to_string()));
    arguments.insert(
        "dir".to_string(),
        Value::String(workspace.display().to_string()),
    );
    arguments.insert("labels".to_string(), labels);
    let spawned = call_tool(mcp, id, "agent_run", Value::Object(arguments));
    assert!(spawned["error"].is_null());
    spawned["result"]["structuredContent"]["session"]["id"]
        .as_str()
        .or_panic("spawn returns session id")
        .to_string()
}

pub(crate) fn tool_names(tools: &Value) -> Vec<&str> {
    tools
        .as_array()
        .or_panic("tools is array")
        .iter()
        .map(|tool| tool["name"].as_str().or_panic("tool name"))
        .collect()
}

pub(crate) fn find_tool<'a>(tools: &'a Value, name: &str) -> &'a Value {
    tools
        .as_array()
        .or_panic("tools is array")
        .iter()
        .find(|tool| tool["name"] == name)
        .unwrap_or_else(|| panic!("missing tool {name}"))
}

pub(crate) fn create_namespace(daemon: &DaemonFixture, name: &str) {
    let status = daemon
        .command()
        .args(["create", "namespace", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .or_panic("namespace create runs");
    assert!(status.success());
}

pub(crate) fn assert_deprecation_hint(tools: &Value, deprecated: &str, replacement: &str) {
    let description = tools
        .as_array()
        .or_panic("tools is array")
        .iter()
        .find(|tool| tool["name"] == deprecated)
        .or_panic("deprecated tool exists")["description"]
        .as_str()
        .or_panic("description is string");
    assert!(description.contains("Deprecated compatibility alias"));
    assert!(description.contains(replacement));
}

pub(crate) fn assert_session_ids(response: &Value, expected: &[&str]) {
    assert!(response["error"].is_null());
    let mut actual = response["result"]["structuredContent"]["sessions"]
        .as_array()
        .or_panic("sessions is array")
        .iter()
        .map(|session| {
            session["id"]
                .as_str()
                .or_panic("session id is string")
                .to_string()
        })
        .collect::<Vec<_>>();
    let mut expected = expected
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}

pub(crate) fn assert_nudged_ids(response: &Value, expected: &[&str]) {
    assert!(response["error"].is_null());
    let mut actual = response["result"]["structuredContent"]["nudges"]
        .as_array()
        .or_panic("nudges is array")
        .iter()
        .map(|nudge| {
            nudge["to"]
                .as_str()
                .or_panic("nudge target is string")
                .to_string()
        })
        .collect::<Vec<_>>();
    let mut expected = expected
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}

pub(crate) fn audited_flow_actions(rows: &[AuditRow]) -> Vec<Action> {
    rows.iter()
        .filter(|row| {
            row.action != Action::List
                && (row.session_ref.is_some()
                    || matches!(row.action, Action::Doctor | Action::Nudge))
        })
        .map(|row| row.action)
        .collect()
}
