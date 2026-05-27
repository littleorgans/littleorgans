mod common;

use common::OrPanic as _;

const SELECTOR_FORMS: &[&str] = &[
    "all",
    "<uuid>",
    "id:<uuid>",
    "role:<name>",
    "namespace:<slug>",
    "dir:<path>",
    "label:<key>=<value>",
    "label:<key> in (v1, v2)",
];

#[test]
fn mcp_each_tool_snapshot() {
    let tool_list = lilo_session_app::mcp::schema::tool_list();
    let tools = tool_list["tools"].as_array().or_panic("tools are an array");
    for tool in tools {
        let name = tool["name"].as_str().or_panic("tool has a name");
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_json_snapshot!("mcp_tool", tool);
        });
    }
}

#[test]
fn mcp_server_instructions_snapshot() {
    insta::assert_snapshot!(
        "mcp_server_instructions",
        lilo_session_app::mcp::instructions::SERVER_INSTRUCTIONS
    );
}

#[test]
fn mcp_surfaces_include_shared_selector_grammar() {
    let instructions = lilo_session_app::mcp::instructions::SERVER_INSTRUCTIONS;
    let tool_list = lilo_session_app::mcp::schema::tool_list();
    let tools = tool_list["tools"].as_array().or_panic("tools are an array");
    let session_list = tools
        .iter()
        .find(|tool| tool["name"] == "session_list")
        .or_panic("session_list schema exists");
    let selector_description = session_list["inputSchema"]["properties"]["selector"]["description"]
        .as_str()
        .or_panic("session_list selector has a description");

    for form in SELECTOR_FORMS {
        assert!(
            instructions.contains(form),
            "server instructions missing selector form {form}"
        );
        assert!(
            selector_description.contains(form),
            "session_list selector description missing selector form {form}"
        );
    }
}
