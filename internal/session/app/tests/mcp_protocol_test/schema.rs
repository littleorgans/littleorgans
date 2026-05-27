#[test]
pub(crate) fn generated_schema_matches_contract_registry() {
    assert_eq!(
        lilo_session_app::mcp::schema::tool_list(),
        lilo_session_app::tool_contracts::contract_registry().tool_list_value()
    );
}
