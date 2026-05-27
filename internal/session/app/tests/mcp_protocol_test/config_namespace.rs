use crate::common::{DaemonFixture, OrPanic as _};
use crate::{call_tool, create_namespace};
use serde_json::{Value, json};

#[tokio::test]
pub(crate) async fn session_run_agent_config_path_is_canonicalized_against_request_dir() {
    let runtime_path = crate::common::fake_runtime_path("codex");
    let daemon = DaemonFixture::start_with_runtime_path(runtime_path.path());
    let workspace = daemon.dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).or_panic("workspace dir");
    let config = workspace.join("agent.toml");
    std::fs::write(&config, "[env]\nHELIOY_AGENT_NAME = \"mcp\"\n").or_panic("agent config");
    let mut mcp = daemon.spawn_mcp();
    mcp.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));

    let spawned = call_tool(
        &mut mcp,
        2,
        "session_run",
        json!({
            "runtime": "codex",
            "role": "engineer",
            "dir": workspace.display().to_string(),
            "agent_config": "./agent.toml"
        }),
    );

    assert!(spawned["error"].is_null(), "{spawned:#}");
    assert_eq!(
        spawned["result"]["structuredContent"]["session"]["agent_config"],
        Value::String(
            std::fs::canonicalize(&config)
                .or_panic("canonical agent config")
                .display()
                .to_string()
        )
    );
}

#[tokio::test]
pub(crate) async fn namespace_tools_list_and_get_records() {
    let daemon = DaemonFixture::start();
    create_namespace(&daemon, "alpha");
    let mut mcp = daemon.spawn_mcp();
    mcp.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));

    let listed = call_tool(&mut mcp, 2, "namespace_list", json!({}));
    assert!(listed["error"].is_null());
    assert_eq!(
        listed["result"]["structuredContent"]["namespaces"][0]["namespace"],
        "alpha"
    );
    assert_eq!(
        listed["result"]["structuredContent"]["namespaces"][1]["namespace"],
        "default"
    );

    let listed_one = call_tool(&mut mcp, 3, "namespace_list", json!({ "slug": "alpha" }));
    assert!(listed_one["error"].is_null());
    assert_eq!(
        listed_one["result"]["structuredContent"]["namespaces"][0]["namespace"],
        "alpha"
    );

    let got = call_tool(&mut mcp, 4, "namespace_get", json!({ "slug": "alpha" }));
    assert!(got["error"].is_null());
    assert_eq!(
        got["result"]["structuredContent"]["namespace"]["namespace"],
        "alpha"
    );
}
