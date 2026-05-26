use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Value, json};

static CONTRACT_REGISTRY: OnceLock<ToolRegistry> = OnceLock::new();
const TOOLS_TOML: &str = include_str!("../tools.toml");

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ToolRegistry {
    pub tools: BTreeMap<String, ToolContract>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ToolContract {
    pub cli_name: String,
    pub cli_about: String,
    pub mcp_description: String,
    pub args_type: String,
    pub response_type: String,
    pub response_description: String,
    #[serde(default)]
    pub params: Vec<ToolParam>,
    #[serde(default)]
    pub outputs: Vec<ToolOutput>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ToolParam {
    pub name: String,
    pub kind: SchemaKind,
    pub required: bool,
    pub mcp_description: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub items_kind: Option<SchemaKind>,
    #[serde(default)]
    pub items_format: Option<String>,
    #[serde(default)]
    pub cli_flag: Option<String>,
    #[serde(default)]
    pub cli_help: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ToolOutput {
    pub name: String,
    pub kind: SchemaKind,
    pub description: String,
    #[serde(default)]
    pub items_kind: Option<SchemaKind>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SchemaKind {
    Array,
    Boolean,
    Integer,
    Object,
    String,
}

pub fn contract_registry() -> &'static ToolRegistry {
    CONTRACT_REGISTRY.get_or_init(|| {
        toml::from_str(TOOLS_TOML).expect("tools.toml must parse as runtime tool contracts")
    })
}

impl ToolRegistry {
    pub fn tool_list_value(&self) -> Value {
        json!({
            "tools": self
                .tools
                .iter()
                .map(|(name, contract)| contract.tool_entry_value(name))
                .collect::<Vec<_>>()
        })
    }

    pub fn admin_tools_markdown(&self) -> String {
        let mut lines = vec![
            "## Admin MCP Tools".to_owned(),
            String::new(),
            "| Tool | Purpose |".to_owned(),
            "| --- | --- |".to_owned(),
        ];
        for (name, contract) in &self.tools {
            lines.push(format!("| `{name}` | {} |", contract.mcp_description));
        }
        lines.push(String::new());
        lines.join("\n")
    }
}

impl ToolContract {
    pub fn tool_entry_value(&self, name: &str) -> Value {
        let mut entry = json!({
            "name": name,
            "description": self.mcp_description,
            "inputSchema": self.input_schema_value()
        });
        if !self.outputs.is_empty() {
            entry["outputSchema"] = self.output_schema_value();
        }
        entry
    }

    pub fn input_schema_value(&self) -> Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();
        for param in &self.params {
            properties.insert(param.name.clone(), param.schema_value());
            if param.required {
                required.push(Value::String(param.name.clone()));
            }
        }
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        })
    }

    pub fn output_schema_value(&self) -> Value {
        let mut properties = serde_json::Map::new();
        for output in &self.outputs {
            properties.insert(output.name.clone(), output.schema_value());
        }
        json!({
            "type": "object",
            "description": self.response_description,
            "properties": properties,
            "additionalProperties": false
        })
    }
}

impl ToolParam {
    fn schema_value(&self) -> Value {
        let mut schema = kind_schema(
            &self.kind,
            self.format.as_deref(),
            self.items_kind.as_ref(),
            self.items_format.as_deref(),
        );
        schema["description"] = Value::String(self.mcp_description.clone());
        schema
    }
}

impl ToolOutput {
    fn schema_value(&self) -> Value {
        let mut schema = kind_schema(&self.kind, None, self.items_kind.as_ref(), None);
        schema["description"] = Value::String(self.description.clone());
        schema
    }
}

fn kind_schema(
    kind: &SchemaKind,
    format: Option<&str>,
    items_kind: Option<&SchemaKind>,
    items_format: Option<&str>,
) -> Value {
    let mut schema = json!({ "type": kind.as_json_type() });
    if let Some(format) = format {
        schema["format"] = Value::String(format.to_owned());
    }
    if let (SchemaKind::Array, Some(items_kind)) = (kind, items_kind) {
        let mut items = json!({ "type": items_kind.as_json_type() });
        if let Some(items_format) = items_format {
            items["format"] = Value::String(items_format.to_owned());
        }
        schema["items"] = items;
    }
    schema
}

impl SchemaKind {
    fn as_json_type(&self) -> &'static str {
        match self {
            Self::Array => "array",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Object => "object",
            Self::String => "string",
        }
    }
}
