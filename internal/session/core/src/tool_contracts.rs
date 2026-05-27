#![allow(dead_code)]

mod contract;
mod metadata;
mod params;
mod raw;
mod registry;
mod render;

pub use contract::{SelectorGrammar, SharedContent, SkillConfig, ToolContract};
pub use metadata::{ArtifactRenderMetadata, CliMetadata};
pub use params::ToolParamContract;
pub use registry::{ToolContractRegistry, contract_registry};
pub use render::{render_selector_grammar_block, rust_const_name};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ErrOrPanic as _, OrPanic as _};

    #[test]
    fn bundled_registry_preserves_committed_tool_order() {
        let names = contract_registry()
            .tools()
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>();

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
                "doctor",
            ]
        );
    }

    #[test]
    fn duplicate_final_mcp_tool_names_are_rejected() {
        let error = ToolContractRegistry::from_toml_str(
            r#"
[tools.first]
cli_name = "first"
mcp_description = "first tool"
cli_about = "first tool"

[[tools.first.mcp_aliases]]
name = "second"
mcp_description = "duplicate alias"

[tools.second]
cli_name = "second"
mcp_description = "second tool"
cli_about = "second tool"
"#,
        )
        .err_or_panic("duplicate alias should fail");

        assert!(error.contains("duplicate MCP tool name second"));
    }

    #[test]
    fn parse_error_points_to_tool_source_layout() {
        let error =
            ToolContractRegistry::from_toml_str("[tools").err_or_panic("invalid TOML should fail");

        assert!(error.contains("failed to parse tools/*.toml"));
    }

    #[test]
    fn duplicate_cli_help_constants_are_rejected() {
        let error = ToolContractRegistry::from_toml_str(
            r#"
[tools.foo_bar]
cli_name = "foo-bar"
mcp_description = "first tool"
cli_about = "first tool"

[tools.foo-bar]
cli_name = "foo bar"
mcp_description = "second tool"
cli_about = "second tool"
"#,
        )
        .err_or_panic("duplicate generated constant should fail");

        assert!(error.contains("duplicate CLI help constant FOO_BAR_ABOUT"));
    }

    #[test]
    fn render_cli_help_false_skips_cli_constant_collision() {
        let registry = ToolContractRegistry::from_toml_str(
            r#"
[tools.foo_bar]
cli_name = "foo-bar"
mcp_description = "first tool"
cli_about = "first tool"

[tools.foo-bar]
cli_name = "foo bar"
mcp_description = "second tool"
cli_about = "second tool"
render_cli_help = false
"#,
        )
        .or_panic("non-rendered CLI help does not collide");

        let skipped = registry
            .tools()
            .iter()
            .find(|tool| tool.name == "foo-bar")
            .or_panic("second tool is present");
        assert!(!skipped.artifacts.render_cli_help);
    }
}
