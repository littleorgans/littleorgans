use crate::common::{self, DaemonFixture, OrPanic as _};
use crate::{audited_flow_actions, call_tool, spawn_agent};
use lilo_im_core::{Action, AuditDecision, AuditRow};
use serde_json::json;
use std::time::{Duration, Instant};

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
pub(crate) async fn tools_call_can_send_read_check_mail_and_nudge() {
    let runtime_path = common::fake_runtime_path("codex");
    let daemon = DaemonFixture::start_with_runtime_path(runtime_path.path());
    let mut mcp = daemon.spawn_mcp();
    mcp.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));

    let _sender = spawn_agent(&mut mcp, 2, "pm", daemon.dir.path());
    let recipient = spawn_agent(&mut mcp, 3, "engineer", daemon.dir.path());

    let sent = call_tool(
        &mut mcp,
        4,
        "mail_send",
        json!({
            "to": recipient.clone(),
            "content": "review the spec",
            "context_id": "review-thread",
            "intent": "request"
        }),
    );
    assert!(sent["error"].is_null());
    assert_eq!(
        sent["result"]["structuredContent"]["results"][0]["message"]["content"],
        "review the spec"
    );

    let checked = call_tool(
        &mut mcp,
        5,
        "mail_check",
        json!({ "selector": format!("id:{recipient}") }),
    );
    assert!(checked["error"].is_null());
    assert_eq!(checked["result"]["structuredContent"]["unread"], 1);
    assert_eq!(
        checked["result"]["structuredContent"]["counts"][0]["unread"],
        1
    );
    assert_eq!(
        checked["result"]["structuredContent"]["counts"][0]["session_id"],
        recipient
    );

    assert_operator_observes_mail(&mut mcp, &recipient);

    let mut recipient_mcp = daemon.spawn_mcp_for_session(&recipient, daemon.dir.path());
    recipient_mcp.send(&json!({"jsonrpc": "2.0", "id": 12, "method": "initialize", "params": {}}));
    let read = call_tool(&mut recipient_mcp, 13, "mail_read", json!({}));
    assert!(read["error"].is_null());
    assert_eq!(
        read["result"]["structuredContent"]["messages"][0]["content"],
        "review the spec"
    );

    let checked = call_tool(
        &mut mcp,
        14,
        "mail_stop_check",
        json!({ "selector": format!("id:{recipient}") }),
    );
    assert!(checked["error"].is_null());
    assert_eq!(checked["result"]["structuredContent"]["unread"], 0);
    assert_eq!(
        checked["result"]["structuredContent"]["counts"][0]["unread"],
        0
    );

    let nudged = call_tool(
        &mut mcp,
        15,
        "nudge",
        json!({ "to": recipient.clone(), "content": "ping" }),
    );
    assert!(nudged["error"].is_null());
    assert_eq!(
        nudged["result"]["structuredContent"]["nudges"][0]["message"],
        "headless runtime does not support nudges"
    );
    assert_eq!(
        nudged["result"]["structuredContent"]["nudges"][0]["delivered"],
        false
    );
    assert!(
        nudged["result"]["structuredContent"]["errors"]
            .as_array()
            .or_panic("nudge errors is array")
            .is_empty()
    );

    assert_mail_flow_audit(&daemon.audit_rows().await);
}

fn assert_operator_observes_mail(mcp: &mut common::McpFixture, recipient: &str) {
    let filter = json!({
        "context_id": "review-thread",
        "recipient": format!("id:{recipient}")
    });
    let peeked = call_tool(mcp, 6, "mail_peek", filter.clone());
    assert!(peeked["error"].is_null());
    assert_eq!(
        peeked["result"]["structuredContent"]["messages"][0]["content"],
        "review the spec"
    );

    let tailed = call_tool(mcp, 7, "mail_tail", with_timeout(filter, 0));
    assert!(tailed["error"].is_null());
    assert_eq!(
        tailed["result"]["structuredContent"]["messages"][0]["recipient"]["session_id"],
        recipient
    );

    let missing_filter = json!({ "context_id": "missing-thread" });
    let snapshot = call_tool(mcp, 8, "mail_tail", missing_filter.clone());
    assert_empty_tail(&snapshot);

    let timeout_zero = call_tool(mcp, 9, "mail_tail", with_timeout(missing_filter.clone(), 0));
    assert_empty_tail(&timeout_zero);

    let started = Instant::now();
    let bounded = call_tool(mcp, 10, "mail_tail", with_timeout(missing_filter, 1));
    assert_empty_tail(&bounded);
    assert!(started.elapsed() < Duration::from_secs(3));
}

fn with_timeout(mut filter: serde_json::Value, seconds: u64) -> serde_json::Value {
    filter["timeout"] = json!(seconds);
    filter
}

fn assert_empty_tail(response: &serde_json::Value) {
    assert!(response["error"].is_null());
    assert!(
        response["result"]["structuredContent"]["messages"]
            .as_array()
            .or_panic("mail tail messages is array")
            .is_empty()
    );
}

fn assert_mail_flow_audit(rows: &[AuditRow]) {
    let actions = audited_flow_actions(rows);
    assert_eq!(
        actions,
        vec![
            Action::Spawn,
            Action::ShimCallback,
            Action::ShimCallback,
            Action::Spawn,
            Action::ShimCallback,
            Action::ShimCallback,
            Action::MailSend,
            Action::MailRead,
            Action::Nudge,
        ]
    );
    assert!(rows.iter().all(|row| row.decision == AuditDecision::Allow));
}
