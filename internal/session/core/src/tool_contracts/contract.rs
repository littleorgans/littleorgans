use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::metadata::{ArtifactRenderMetadata, CliMetadata};
use super::params::{ToolParamContract, mcp_namespace_scope_params};
use super::raw::{RawToolAlias, RawToolDef, parse_optional_json};

#[derive(Debug, Clone, Deserialize)]
pub struct SkillConfig {
    pub workflow: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SharedContent {
    pub selector_grammar: Option<SelectorGrammar>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelectorGrammar {
    pub forms: Vec<String>,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ToolContract {
    pub name: String,
    pub mcp_description: String,
    pub cli: CliMetadata,
    pub params: Vec<ToolParamContract>,
    pub output_schema: Option<Value>,
    pub artifacts: ArtifactRenderMetadata,
}

impl ToolContract {
    pub(super) fn from_raw(name: String, raw: RawToolDef) -> Result<Self, String> {
        let mut params = raw
            .params
            .into_iter()
            .map(|param| ToolParamContract::from_raw(&name, param))
            .collect::<Result<Vec<_>, _>>()?;
        if raw.mcp_namespace_scope {
            params.extend(mcp_namespace_scope_params());
        }
        let output_schema = parse_optional_json(&name, "output_schema", raw.output_schema)?;

        Ok(Self {
            artifacts: ArtifactRenderMetadata::for_tool(&name, raw.render_cli_help),
            cli: CliMetadata {
                name: raw.cli_name,
                about: raw.cli_about,
            },
            mcp_description: raw.mcp_description,
            name,
            output_schema,
            params,
        })
    }

    pub fn tool_entry_value(&self, shared: &SharedContent) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();
        for param in &self.params {
            properties.insert(param.name.clone(), param.schema_value(shared));
            if param.required {
                required.push(Value::String(param.name.clone()));
            }
        }

        let mut input_schema = json!({
            "additionalProperties": false,
            "type": "object",
            "properties": properties
        });
        if !required.is_empty() {
            input_schema["required"] = Value::Array(required);
        }

        let mut entry = json!({
            "name": self.name,
            "description": self.mcp_description,
            "inputSchema": input_schema
        });
        if let Some(schema) = &self.output_schema {
            entry["outputSchema"] = schema.clone();
        }
        entry
    }

    pub(super) fn alias(&self, raw: RawToolAlias) -> Result<Self, String> {
        if raw.name.trim().is_empty() {
            return Err(format!("{}.mcp_aliases name must not be empty", self.name));
        }
        Ok(Self {
            artifacts: ArtifactRenderMetadata::for_alias(&raw.name),
            cli: self.cli.clone(),
            mcp_description: raw.mcp_description,
            name: raw.name,
            output_schema: self.output_schema.clone(),
            params: self.params.clone(),
        })
    }
}
