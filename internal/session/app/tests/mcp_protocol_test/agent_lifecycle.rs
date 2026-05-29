use crate::common::{self, DaemonFixture, OrPanic as _};
use crate::{audited_flow_actions, call_tool};
use lilo_im_core::{Action, AuditDecision};
use serde_json::json;

#[tokio::test]
pub(crate) async fn tools_call_can_run_list_get_and_delete_agent() {
    let runtime_path = common::fake_runtime_path("codex");
    let daemon = DaemonFixture::start_with_runtime_path(runtime_path.path());
    let mut mcp = daemon.spawn_mcp();
    mcp.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));

    assert_empty_agent_list(&mut mcp);
    assert_agent_run_requires_dir(&mut mcp, &daemon);
    let id = spawn_mcp_agent(&mut mcp, &daemon);
    assert_agent_get(&mut mcp, &daemon, &id);
    assert_capture_tools(&mut mcp, &id);
    assert_wait_and_doctor(&mut mcp, &id);
    assert_agent_delete(&mut mcp, &id);
    assert_delete_flow_audit(&daemon).await;
}

pub(crate) fn assert_empty_agent_list(mcp: &mut common::McpFixture) {
    let empty = call_tool(mcp, 2, "agent_list", json!({}));
    assert!(empty["error"].is_null());
    assert_eq!(
        empty["result"]["structuredContent"]["sessions"]
            .as_array()
            .or_panic("sessions is array")
            .len(),
        0
    );
}

pub(crate) fn assert_agent_run_requires_dir(mcp: &mut common::McpFixture, daemon: &DaemonFixture) {
    let alias_only = call_tool(
        mcp,
        3,
        "agent_run",
        json!({
            "runtime": "codex",
            "role": "engineer",
            "workspace": daemon.dir.path().display().to_string()
        }),
    );
    assert_eq!(
        alias_only["result"]["_meta"]["sm_tool_error"]["message"],
        "missing required argument `dir`"
    );
}

pub(crate) fn spawn_mcp_agent(mcp: &mut common::McpFixture, daemon: &DaemonFixture) -> String {
    let spawned = call_tool(
        mcp,
        4,
        "agent_run",
        json!({
            "runtime": "codex",
            "role": "engineer",
            "dir": daemon.dir.path().display().to_string()
        }),
    );
    assert!(spawned["error"].is_null());
    spawned["result"]["structuredContent"]["session"]["id"]
        .as_str()
        .or_panic("spawn returns session id")
        .to_string()
}

pub(crate) fn assert_agent_get(mcp: &mut common::McpFixture, daemon: &DaemonFixture, id: &str) {
    let found = call_tool(mcp, 5, "agent_get", json!({ "id": id }));
    assert!(found["error"].is_null());
    assert_eq!(
        found["result"]["structuredContent"]["session"]["workspace"],
        daemon.dir.path().display().to_string()
    );
}

pub(crate) fn assert_capture_tools(mcp: &mut common::McpFixture, id: &str) {
    let capture = call_tool(
        mcp,
        11,
        "session_capture",
        json!({ "id": id, "scrollback_lines": 20 }),
    );
    assert!(capture["error"].is_null());
    assert_eq!(
        capture["result"]["structuredContent"]["capture"]["status"],
        "failed"
    );

    let broad_capture = call_tool(mcp, 11, "session_capture", json!({ "selector": "all" }));
    assert!(broad_capture["error"].is_null());
    assert_eq!(
        broad_capture["result"]["_meta"]["sm_tool_error"]["is_error"],
        true
    );
    assert!(
        broad_capture["result"]["_meta"]["sm_tool_error"]["message"]
            .as_str()
            .or_panic("error message is string")
            .contains("missing required argument `id`")
    );
}

pub(crate) fn assert_wait_and_doctor(mcp: &mut common::McpFixture, id: &str) {
    let waited = call_tool(
        mcp,
        7,
        "wait",
        json!({ "selector": format!("id:{id}"), "for": "running", "timeout_secs": 0 }),
    );
    assert!(waited["error"].is_null());
    assert_eq!(waited["result"]["structuredContent"]["matched"], true);

    let doctor = call_tool(mcp, 8, "doctor", json!({}));
    assert!(doctor["error"].is_null());
    assert_eq!(
        doctor["result"]["structuredContent"]["runtime"],
        "rtmd (lilo-rm-client 0.6.x, protocol 0.6)"
    );
    assert_eq!(
        doctor["result"]["structuredContent"]["runtime_matters"]["status"],
        "ok"
    );
}

pub(crate) fn assert_agent_delete(mcp: &mut common::McpFixture, id: &str) {
    let deleted = call_tool(
        mcp,
        9,
        "agent_delete",
        json!({ "selector": format!("id:{id}"), "signal": "SIGTERM", "grace_secs": 1 }),
    );
    assert!(deleted["error"].is_null());
    assert_eq!(
        deleted["result"]["structuredContent"]["sessions"][0]["state"],
        "TERMINATED"
    );
    assert!(
        deleted["result"]["structuredContent"]["errors"]
            .as_array()
            .or_panic("errors is array")
            .is_empty()
    );
}

pub(crate) async fn assert_delete_flow_audit(daemon: &DaemonFixture) {
    let rows = daemon.audit_rows().await;
    let actions = audited_flow_actions(&rows);
    assert_eq!(
        actions,
        vec![
            Action::Spawn,
            Action::Spawn,
            Action::ShimCallback,
            Action::ShimCallback,
            Action::Read,
            Action::Doctor,
            Action::Doctor,
            Action::Kill,
            Action::ShimCallback,
        ]
    );
    assert!(rows.iter().all(|row| row.decision == AuditDecision::Allow));
}
