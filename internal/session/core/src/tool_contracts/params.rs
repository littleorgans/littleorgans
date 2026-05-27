use serde_json::{Map, Value, json};

use super::contract::SharedContent;
use super::raw::{RawParamDef, parse_json};
use super::render::render_selector_grammar_block;

#[derive(Debug, Clone)]
pub struct ToolParamContract {
    pub name: String,
    pub required: bool,
    pub enum_values: Option<Vec<String>>,
    pub mcp_description: String,
    pub cli_help: Option<String>,
    pub cli_flag: Option<String>,
    pub selector: bool,
    shape: ParamShape,
}

impl ToolParamContract {
    pub(super) fn from_raw(tool_name: &str, raw: RawParamDef) -> Result<Self, String> {
        Ok(Self {
            shape: ParamShape::from_raw(tool_name, &raw)?,
            name: raw.name,
            required: raw.required,
            enum_values: raw.enum_values,
            mcp_description: raw.mcp_description,
            cli_help: raw.cli_help,
            cli_flag: raw.cli_flag,
            selector: raw.selector,
        })
    }

    pub fn schema_value(&self, shared: &SharedContent) -> Value {
        let mut schema = self.shape.schema_object();
        let description = if self.selector {
            let Some(selector_help) = render_selector_grammar_block(shared) else {
                panic!("shared.selector_grammar exists for selector MCP params");
            };
            format!("{}\n\n{selector_help}", self.mcp_description)
        } else {
            self.mcp_description.clone()
        };
        schema.insert("description".to_string(), Value::String(description));
        if let Some(values) = &self.enum_values {
            schema.insert(
                "enum".to_string(),
                Value::Array(values.iter().cloned().map(Value::String).collect()),
            );
        }
        Value::Object(schema)
    }
}

#[derive(Debug, Clone)]
enum ParamShape {
    Scalar(String),
    Array(String),
    Custom(Value),
}

impl ParamShape {
    fn from_raw(tool_name: &str, raw: &RawParamDef) -> Result<Self, String> {
        if let Some(schema) = &raw.mcp_schema {
            return parse_json(tool_name, &raw.name, schema).map(Self::Custom);
        }
        if raw.type_ == "array" {
            return Ok(Self::Array(
                raw.items_type
                    .clone()
                    .unwrap_or_else(|| "string".to_string()),
            ));
        }
        Ok(Self::Scalar(raw.type_.clone()))
    }

    fn schema_object(&self) -> Map<String, Value> {
        match self {
            Self::Scalar(kind) => {
                Map::from_iter([("type".to_string(), Value::String(kind.clone()))])
            }
            Self::Array(item_kind) => Map::from_iter([
                ("type".to_string(), Value::String("array".to_string())),
                ("items".to_string(), json!({ "type": item_kind })),
            ]),
            Self::Custom(value) => value.as_object().cloned().unwrap_or_default(),
        }
    }
}

pub(super) fn mcp_namespace_scope_params() -> Vec<ToolParamContract> {
    vec![
        ToolParamContract {
            name: "namespace".to_string(),
            required: false,
            enum_values: None,
            mcp_description: "Namespace slug to scope this read. Overrides the caller session namespace fallback.".to_string(),
            cli_help: None,
            cli_flag: None,
            selector: false,
            shape: ParamShape::Scalar("string".to_string()),
        },
        ToolParamContract {
            name: "all_namespaces".to_string(),
            required: false,
            enum_values: None,
            mcp_description: "Bypass namespace scoping and read across all namespaces.".to_string(),
            cli_help: None,
            cli_flag: None,
            selector: false,
            shape: ParamShape::Scalar("boolean".to_string()),
        },
    ]
}
