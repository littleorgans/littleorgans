use crate::common::{self, DaemonFixture, OrPanic as _};
use crate::{audited_flow_actions, call_tool, spawn_agent};
use lilo_im_core::{Action, AuditDecision, AuditRow};
use serde_json::json;

#[tokio::test]
pub(crate) async fn tools_call_can_send_read_check_mail_and_nudge() {
    let runtime_path = common::fake_runtime_path("codex");
    let daemon = DaemonFixture::start_with_runtime_path(runtime_path.path());
    let mut mcp = daemon.spawn_mcp();
    mcp.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));

    let sender = spawn_agent(&mut mcp, 2, "pm", daemon.dir.path());
    let recipient = spawn_agent(&mut mcp, 3, "engineer", daemon.dir.path());

    let sent = call_tool(
        &mut mcp,
        4,
        "mail_send",
        json!({
            "from": sender,
            "to": recipient.clone(),
            "content": "review the spec"
        }),
    );
    assert!(sent["error"].is_null());
    assert_eq!(
        sent["result"]["structuredContent"]["mail"][0]["content"],
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

    let read = call_tool(
        &mut mcp,
        6,
        "mail_read",
        json!({ "selector": format!("id:{recipient}") }),
    );
    assert!(read["error"].is_null());
    assert_eq!(
        read["result"]["structuredContent"]["mail"][0]["content"],
        "review the spec"
    );

    let checked = call_tool(
        &mut mcp,
        7,
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
        8,
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

fn assert_mail_flow_audit(rows: &[AuditRow]) {
    let actions = audited_flow_actions(rows);
    assert_eq!(
        actions,
        vec![
            Action::Spawn,
            Action::Spawn,
            Action::ShimCallback,
            Action::ShimCallback,
            Action::Spawn,
            Action::Spawn,
            Action::ShimCallback,
            Action::ShimCallback,
            Action::MailSend,
            Action::MailRead,
            Action::Nudge,
            Action::Nudge,
        ]
    );
    assert!(rows.iter().all(|row| row.decision == AuditDecision::Allow));
}
