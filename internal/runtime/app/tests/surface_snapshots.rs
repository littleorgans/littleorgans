#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::process::{Command, Output};

use common::mcp::{call_tool, mcp_json, request};
use common::{RtmHarness, output_stderr, output_stdout, spawn_ok, wait_for_events};
use lilo_rm_core::{RuntimeResponse, RuntimeRpc};
use lilo_runtime_app::cli::output::redact_cli_json_snapshot;
use serde_json::{Value, json};
use uuid::Uuid;

#[test]
fn status_json_output_is_stable() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    spawn_ok(&harness, &session_id, "claude");

    let output = harness.status_format(&session_id, "json");
    assert!(output.status.success(), "status json failed: {output:?}");
    lilo_runtime_app::assert_cli_json_snapshot!(output_stdout(output));
}

#[test]
fn session_facing_cli_json_outputs_are_stable() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();

    let spawn = harness.cli(&[
        "spawn",
        "--runtime",
        "claude",
        "--session-id",
        &session_id,
        "--target",
        "headless",
    ]);
    assert!(spawn.status.success(), "spawn failed: {spawn:?}");

    let nudge = harness.cli(&["nudge", &session_id, "--content", "hello"]);
    assert!(!nudge.status.success(), "nudge unexpectedly succeeded");

    let kill = harness.kill(&session_id, "TERM", 2);
    assert!(kill.status.success(), "kill failed: {kill:?}");
    wait_for_events(&harness, 2);

    let version = json_stdout(harness.cli(&["version"]));
    let doctor = json_stdout(harness.cli(&["doctor"]));
    let status = json_stdout(harness.cli(&["status", "--session-id", &session_id]));
    let events = json_stdout(harness.cli(&["events"]));
    let spawn = json_from_output(&output_stdout(spawn));
    let kill = json_from_output(&output_stdout(kill));
    let nudge_error = json_from_output(&output_stderr(nudge));

    let mut snapshot = json!({
        "version": version,
        "doctor": doctor,
        "status": status,
        "events": events,
        "spawn": spawn,
        "kill": kill,
        "nudge_error": nudge_error,
    });
    redact_cli_json_snapshot(&mut snapshot);

    insta::assert_json_snapshot!(snapshot);
}

#[test]
fn doctor_output_is_stable() {
    let harness = RtmHarness::start();
    let output = harness.cli(&["doctor", "--format", "human"]);
    assert!(output.status.success(), "doctor failed: {output:?}");

    insta::assert_snapshot!(normalize_doctor(&output_stdout(output)));
}

#[test]
fn doctor_json_response_is_stable() {
    let harness = RtmHarness::start();
    let response = tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(lilo_runtime_app::shared::request(
            harness.socket_path(),
            RuntimeRpc::Doctor,
        ))
        .expect("doctor rpc");

    let RuntimeResponse::Doctor(payload) = response else {
        panic!("unexpected doctor response: {response:?}");
    };
    let mut doctor = serde_json::to_value(payload.doctor).expect("doctor json");
    redact_cli_json_snapshot(&mut doctor);

    insta::assert_json_snapshot!(doctor);
}

#[test]
fn mcp_responses_are_stable() {
    let harness = RtmHarness::start();
    let initialize = mcp_json(&harness, request(1, "initialize", json!({})));
    let tools = mcp_json(&harness, request(2, "tools/list", json!({})));
    let status = call_tool(&harness, 3, "rtm_status", json!({}));
    let mut version = call_tool(&harness, 4, "rtm_version", json!({}));
    let watchers = call_tool(&harness, 5, "rtm_watchers", json!({}));

    let session_id = Uuid::now_v7().to_string();
    let spawn = spawn_ok(&harness, &session_id, "claude");
    let runtime_pid = common::parse_runtime_pid(&spawn);
    let mut kill = call_tool(
        &harness,
        6,
        "rtm_kill_by_pid",
        json!({
            "pid": runtime_pid,
            "signal": 15,
            "grace_secs": 0
        }),
    );
    normalize_mcp_payload_text(&mut version);
    normalize_mcp_payload_text(&mut kill);
    let mut snapshot = json!({
        "initialize": initialize,
        "tools": tools,
        "status": status,
        "version": version,
        "watchers": watchers,
        "kill_by_pid": kill,
    });
    redact_cli_json_snapshot(&mut snapshot);

    insta::assert_json_snapshot!(snapshot);
}

fn json_stdout(output: Output) -> Value {
    assert!(output.status.success(), "json command failed: {output:?}");
    json_from_output(&output_stdout(output))
}

fn json_from_output(output: &str) -> Value {
    jq_round_trip(output);
    serde_json::from_str(output).expect("json output")
}

fn jq_round_trip(output: &str) {
    let jq = Command::new("jq")
        .arg("-c")
        .arg(".")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut jq) = jq else {
        return;
    };
    let mut stdin = jq.stdin.take().expect("jq stdin");
    std::io::Write::write_all(&mut stdin, output.as_bytes()).expect("write jq stdin");
    drop(stdin);
    assert!(jq.wait().expect("jq wait").success(), "jq rejected output");
}

fn normalize_mcp_payload_text(response: &mut Value) {
    let mut structured = response["result"]["structuredContent"].clone();
    redact_cli_json_snapshot(&mut structured);
    let text = serde_json::to_string(&structured).expect("structured text");
    response["result"]["content"][0]["text"] = json!(text);
}

fn normalize_doctor(output: &str) -> String {
    output
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("version") {
                "  version             [version]".to_owned()
            } else if trimmed.starts_with("socket") {
                "  socket              [socket]".to_owned()
            } else if trimmed.starts_with("uptime") {
                "  uptime              [uptime]".to_owned()
            } else if trimmed.starts_with("claude") {
                "  claude              [command]".to_owned()
            } else if trimmed.starts_with("codex") {
                "  codex               [command]".to_owned()
            } else if trimmed.starts_with("tmux") {
                "tmux                  [availability]".to_owned()
            } else if trimmed.starts_with("last probe sweep") {
                "last probe sweep      [timestamp]".to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
