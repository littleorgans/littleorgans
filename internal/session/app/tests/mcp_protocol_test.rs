mod common;

#[path = "mcp_protocol_test/agent_lifecycle.rs"]
mod agent_lifecycle;
#[path = "mcp_protocol_test/config_namespace.rs"]
mod config_namespace;
#[path = "mcp_protocol_test/handshake.rs"]
mod handshake;
#[path = "mcp_protocol_test/helpers.rs"]
mod helpers;
#[path = "mcp_protocol_test/mail.rs"]
mod mail;
#[path = "mcp_protocol_test/schema.rs"]
mod schema;
#[path = "mcp_protocol_test/selectors.rs"]
mod selectors;

#[allow(unused_imports)]
pub(crate) use agent_lifecycle::{
    assert_agent_delete, assert_agent_get, assert_agent_run_requires_dir, assert_capture_tools,
    assert_delete_flow_audit, assert_empty_agent_list, assert_wait_and_doctor, spawn_mcp_agent,
    tools_call_can_run_list_get_and_delete_agent,
};
#[allow(unused_imports)]
pub(crate) use config_namespace::{
    namespace_tools_list_and_get_records,
    session_run_agent_config_path_is_canonicalized_against_request_dir,
};
#[allow(unused_imports)]
pub(crate) use handshake::initialize_and_tools_list_follow_mcp_shape;
#[allow(unused_imports)]
pub(crate) use helpers::{
    assert_deprecation_hint, assert_nudged_ids, assert_session_ids, call_tool, create_namespace,
    find_tool, spawn_agent, spawn_agent_in_namespace, spawn_agent_with_labels, tool_names,
};
#[allow(unused_imports)]
pub(crate) use mail::tools_call_can_send_read_check_mail_and_nudge;
#[allow(unused_imports)]
pub(crate) use schema::generated_schema_matches_contract_registry;
#[allow(unused_imports)]
pub(crate) use selectors::{
    session_tools_share_agent_handlers_and_namespace_read_scope,
    tools_call_can_select_and_label_agents,
};
