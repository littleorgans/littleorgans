#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use common::mcp::{call_tool, content_text, mcp_json, request, tool_names};
use common::{RtmHarness, output_stdout, parse_runtime_pid, wait_for_status};
use serde_json::{Value, json};
use uuid::Uuid;

#[test]
fn pass6_mcp_lists_admin_tools_and_reports_status_version_watchers() {
    let harness = RtmHarness::start();
    let tools = mcp_json(&harness, request(1, "tools/list", json!({})));
    let names = tool_names(&tools);
    let generated: Value =
        serde_json::from_str(lilo_runtime_app::generated::mcp_tools::TOOL_LIST_JSON)
            .expect("generated");

    assert_eq!(tools["result"], generated);
    assert_eq!(names, lilo_runtime_app::generated::mcp_tools::TOOL_NAMES);
    assert!(!names.contains(&"spawn"));
    assert!(!names.contains(&"kill"));
    assert!(!names.contains(&"rtm_doctor"));

    let status = call_tool(&harness, 2, "rtm_status", json!({}));
    let status_text = content_text(&status);
    let lifecycles: Vec<Value> = serde_json::from_str(status_text).expect("status rows");
    assert!(lifecycles.is_empty(), "{status_text}");

    let version = call_tool(&harness, 3, "rtm_version", json!({}));
    let version_text: Value = serde_json::from_str(content_text(&version)).expect("version text");
    assert_eq!(version_text["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        version_text["git_sha"]
            .as_str()
            .is_some_and(|sha| !sha.is_empty())
    );

    let watchers = call_tool(&harness, 4, "rtm_watchers", json!({}));
    let watcher_text: Value = serde_json::from_str(content_text(&watchers)).expect("watchers text");
    assert_eq!(watcher_text["process_exit_watchers"], 0);
    assert_eq!(watcher_text["shim_sockets"], 0);

    harness.stop();
}

#[test]
fn pass6_mcp_kill_by_pid_signals_runtime() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    let spawn = harness.spawn(&session_id);
    assert!(spawn.status.success(), "spawn failed: {spawn:?}");
    let pid = parse_runtime_pid(&output_stdout(spawn));

    let result = call_tool(
        &harness,
        1,
        "rtm_kill_by_pid",
        json!({
            "pid": pid,
            "signal": 15,
            "grace_secs": 0
        }),
    );
    let text: Value = serde_json::from_str(content_text(&result)).expect("kill text");
    assert_eq!(text["pid"], pid);
    assert_eq!(text["signal"], 15);
    wait_for_status(&harness, &session_id, "Exited");

    harness.stop();
}
