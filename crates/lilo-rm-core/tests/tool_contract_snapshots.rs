use lilo_rm_core::tool_contracts::contract_registry;

#[test]
fn mcp_tool_list_contract_is_stable() {
    insta::assert_json_snapshot!(contract_registry().tool_list_value());
}

#[test]
fn admin_tools_readme_section_is_stable() {
    insta::assert_snapshot!(contract_registry().admin_tools_markdown());
}
