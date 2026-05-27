use super::render::rust_const_name;

#[derive(Debug, Clone)]
pub struct CliMetadata {
    pub name: String,
    pub about: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactRenderMetadata {
    pub mcp_schema_file: String,
    pub cli_help_prefix: String,
    pub render_cli_help: bool,
}

impl ArtifactRenderMetadata {
    pub(super) fn for_tool(name: &str, render_cli_help: bool) -> Self {
        Self {
            mcp_schema_file: format!("{name}.json"),
            cli_help_prefix: rust_const_name(name),
            render_cli_help,
        }
    }

    pub(super) fn for_alias(name: &str) -> Self {
        Self {
            mcp_schema_file: format!("{name}.json"),
            cli_help_prefix: rust_const_name(name),
            render_cli_help: false,
        }
    }
}
