use crate::common::{DaemonFixture, OrPanic as _};
use crate::{assert_deprecation_hint, find_tool, tool_names};
use serde_json::json;

#[test]
pub(crate) fn initialize_and_tools_list_follow_mcp_shape() {
    let daemon = DaemonFixture::start();
    let mut mcp = daemon.spawn_mcp();

    let initialized = mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    }));
    assert_eq!(initialized["jsonrpc"], "2.0");
    assert_eq!(initialized["id"], 1);
    assert!(initialized["error"].is_null());
    assert_eq!(initialized["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(initialized["result"]["serverInfo"]["name"], "sm");
    assert!(initialized["result"]["instructions"].is_string());
    assert!(initialized["result"]["capabilities"]["tools"].is_object());

    let listed = mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    }));
    assert_eq!(listed["jsonrpc"], "2.0");
    assert_eq!(listed["id"], 2);
    assert!(listed["error"].is_null());
    let names = tool_names(&listed["result"]["tools"]);
    assert_eq!(
        names,
        vec![
            "session_run",
            "agent_run",
            "session_list",
            "agent_list",
            "session_get",
            "agent_get",
            "namespace_list",
            "namespace_get",
            "session_capture",
            "agent_capture",
            "session_delete",
            "agent_delete",
            "session_label",
            "agent_label",
            "mail_send",
            "mail_read",
            "mail_check",
            "mail_stop_check",
            "nudge",
            "logs",
            "wait",
            "doctor"
        ]
    );
    assert_deprecation_hint(&listed["result"]["tools"], "agent_list", "session_list");
    assert_deprecation_hint(&listed["result"]["tools"], "agent_get", "session_get");
    let capture = find_tool(&listed["result"]["tools"], "session_capture");
    assert!(
        capture["inputSchema"]["required"]
            .as_array()
            .or_panic("required is array")
            .contains(&json!("id"))
    );
    assert_eq!(capture["inputSchema"]["properties"]["id"]["format"], "uuid");
    assert!(capture["inputSchema"]["properties"]["selector"].is_null());
}
