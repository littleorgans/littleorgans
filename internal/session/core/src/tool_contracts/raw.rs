use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

use super::contract::{SharedContent, SkillConfig};

#[derive(Debug, Deserialize)]
pub(super) struct RawToolsToml {
    #[serde(default)]
    pub(super) shared: Option<SharedContent>,
    pub(super) skill: Option<SkillConfig>,
    pub(super) tools: IndexMap<String, RawToolDef>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawToolDef {
    pub(super) cli_name: String,
    pub(super) mcp_description: String,
    pub(super) cli_about: String,
    #[serde(default = "default_render_cli_help")]
    pub(super) render_cli_help: bool,
    pub(super) output_schema: Option<String>,
    #[serde(default)]
    pub(super) mcp_namespace_scope: bool,
    #[serde(default)]
    pub(super) params: Vec<RawParamDef>,
    #[serde(default)]
    pub(super) mcp_aliases: Vec<RawToolAlias>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawParamDef {
    pub(super) name: String,
    #[serde(rename = "type")]
    pub(super) type_: String,
    #[serde(default)]
    pub(super) required: bool,
    pub(super) items_type: Option<String>,
    pub(super) enum_values: Option<Vec<String>>,
    pub(super) mcp_description: String,
    pub(super) cli_help: Option<String>,
    pub(super) cli_flag: Option<String>,
    #[serde(default)]
    pub(super) selector: bool,
    pub(super) mcp_schema: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawToolAlias {
    pub(super) name: String,
    pub(super) mcp_description: String,
}

fn default_render_cli_help() -> bool {
    true
}

pub(super) fn parse_optional_json(
    tool_name: &str,
    field: &'static str,
    raw: Option<String>,
) -> Result<Option<Value>, String> {
    raw.map(|schema| parse_json(tool_name, field, &schema))
        .transpose()
}

pub(super) fn parse_json(tool_name: &str, field: &str, raw: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|error| format!("{tool_name}.{field} is not valid JSON: {error}"))?;
    if !value.is_object() {
        return Err(format!("{tool_name}.{field} must be a JSON object"));
    }
    Ok(value)
}
