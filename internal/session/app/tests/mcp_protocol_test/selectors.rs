use crate::common::{DaemonFixture, OrPanic as _};
use crate::{
    assert_nudged_ids, assert_session_ids, call_tool, create_namespace, spawn_agent_in_namespace,
    spawn_agent_with_labels,
};
use serde_json::json;

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
pub(crate) async fn tools_call_can_select_and_label_agents() {
    let runtime_path = crate::common::fake_runtime_path("codex");
    let daemon = DaemonFixture::start_with_runtime_path(runtime_path.path());
    let mut mcp = daemon.spawn_mcp();
    mcp.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));

    let auth = spawn_agent_with_labels(
        &mut mcp,
        2,
        "engineer",
        daemon.dir.path(),
        json!(["area=auth"]),
    );
    let ui = spawn_agent_with_labels(
        &mut mcp,
        3,
        "engineer",
        daemon.dir.path(),
        json!(["area=ui"]),
    );

    let selected = call_tool(
        &mut mcp,
        4,
        "agent_list",
        json!({ "selector": "label:area=auth" }),
    );
    assert!(selected["error"].is_null());
    assert_eq!(
        selected["result"]["structuredContent"]["sessions"][0]["id"],
        auth
    );

    let labeled = call_tool(
        &mut mcp,
        5,
        "agent_label",
        json!({ "selector": format!("id:{ui}"), "mutation": "area=auth" }),
    );
    assert!(labeled["error"].is_null());

    let selected = call_tool(
        &mut mcp,
        6,
        "agent_list",
        json!({ "selector": "label:area=auth" }),
    );
    assert!(selected["error"].is_null());
    assert_eq!(
        selected["result"]["structuredContent"]["sessions"]
            .as_array()
            .or_panic("sessions is array")
            .len(),
        2
    );
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
pub(crate) async fn session_tools_share_agent_handlers_and_namespace_read_scope() {
    let runtime_path = crate::common::fake_runtime_path("codex");
    let daemon = DaemonFixture::start_with_runtime_path(runtime_path.path());
    create_namespace(&daemon, "alpha");
    create_namespace(&daemon, "beta");

    let mut mcp = daemon.spawn_mcp();
    mcp.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));

    let caller = spawn_agent_in_namespace(&mut mcp, 2, "caller", daemon.dir.path(), "alpha");
    let alpha_peer = spawn_agent_in_namespace(&mut mcp, 3, "engineer", daemon.dir.path(), "alpha");
    let beta_peer = spawn_agent_in_namespace(&mut mcp, 4, "engineer", daemon.dir.path(), "beta");

    let agent_all = call_tool(&mut mcp, 5, "agent_list", json!({ "all_namespaces": true }));
    let session_all = call_tool(
        &mut mcp,
        6,
        "session_list",
        json!({ "all_namespaces": true }),
    );
    assert_session_ids(&agent_all, &[&caller, &alpha_peer, &beta_peer]);
    assert_session_ids(&session_all, &[&caller, &alpha_peer, &beta_peer]);

    let explicit_alpha = call_tool(&mut mcp, 7, "session_list", json!({ "namespace": "alpha" }));
    assert_session_ids(&explicit_alpha, &[&caller, &alpha_peer]);

    let bypass = call_tool(
        &mut mcp,
        8,
        "session_list",
        json!({ "all_namespaces": true }),
    );
    assert_session_ids(&bypass, &[&caller, &alpha_peer, &beta_peer]);

    let marked_cwd = daemon.dir.path().join("marked-cwd");
    std::fs::create_dir_all(marked_cwd.join(".sm")).or_panic("marker dir creates");
    std::fs::write(marked_cwd.join(".sm").join("namespace"), "beta").or_panic("marker writes");
    let mut caller_mcp = daemon.spawn_mcp_for_session(&caller, &marked_cwd);
    caller_mcp.send(&json!({"jsonrpc": "2.0", "id": 9, "method": "initialize", "params": {}}));

    let implicit_alpha = call_tool(&mut caller_mcp, 10, "session_list", json!({}));
    assert_session_ids(&implicit_alpha, &[&caller, &alpha_peer]);

    let mailed_alpha = call_tool(
        &mut caller_mcp,
        11,
        "mail_send",
        json!({
            "to": "role:engineer",
            "content": "alpha only",
            "context_id": "scope-thread",
            "intent": "inform"
        }),
    );
    assert_mail_recipient_ids(&mailed_alpha, &[&alpha_peer]);

    let mailed_all = call_tool(
        &mut caller_mcp,
        12,
        "mail_send",
        json!({
            "to": "role:engineer",
            "all_namespaces": true,
            "content": "all",
            "context_id": "scope-thread",
            "intent": "inform"
        }),
    );
    assert_mail_recipient_ids(&mailed_all, &[&alpha_peer, &beta_peer]);

    let nudged_alpha = call_tool(
        &mut mcp,
        13,
        "nudge",
        json!({ "to": "all", "namespace": "alpha", "content": "ping" }),
    );
    assert_nudged_ids(&nudged_alpha, &[&caller, &alpha_peer]);

    let nudged_all = call_tool(
        &mut mcp,
        14,
        "nudge",
        json!({ "to": "all", "all_namespaces": true, "content": "ping" }),
    );
    assert_nudged_ids(&nudged_all, &[&caller, &alpha_peer, &beta_peer]);
}

fn assert_mail_recipient_ids(response: &serde_json::Value, expected: &[&str]) {
    assert!(response["error"].is_null());
    let mut actual = response["result"]["structuredContent"]["results"]
        .as_array()
        .or_panic("mail results is array")
        .iter()
        .filter_map(|result| result["message"]["recipient"]["session_id"].as_str())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut expected = expected
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}
