use std::collections::HashSet;
use std::sync::OnceLock;

use serde_json::{Value, json};

use super::contract::{SharedContent, SkillConfig, ToolContract};
use super::raw::RawToolsToml;
use super::render::rust_const_name;

include!(concat!(env!("OUT_DIR"), "/tool_sources.rs"));
#[path = "../tool_sources.rs"]
mod tool_sources;

static REGISTRY: OnceLock<ToolContractRegistry> = OnceLock::new();

pub fn contract_registry() -> &'static ToolContractRegistry {
    REGISTRY.get_or_init(|| {
        let content = bundled_tool_sources();
        ToolContractRegistry::from_toml_str(&content)
            .unwrap_or_else(|error| panic!("tools/*.toml sources are valid: {error}"))
    })
}

fn bundled_tool_sources() -> String {
    let mut content = String::new();
    for (_, source) in TOOL_SOURCE_FILES {
        tool_sources::append_tool_source(&mut content, source);
    }
    content
}

#[derive(Debug, Clone)]
pub struct ToolContractRegistry {
    shared: SharedContent,
    skill: Option<SkillConfig>,
    tools: Vec<ToolContract>,
}

impl ToolContractRegistry {
    pub fn from_toml_str(content: &str) -> Result<Self, String> {
        let parsed: RawToolsToml = toml::from_str(content)
            .map_err(|error| format!("failed to parse tools/*.toml: {error}"))?;
        let mut raw_tools = parsed.tools.into_iter().enumerate().collect::<Vec<_>>();
        raw_tools.sort_by(
            |(left_index, (left_name, _)), (right_index, (right_name, _))| {
                tool_contract_sort_key(left_name, *left_index)
                    .cmp(&tool_contract_sort_key(right_name, *right_index))
            },
        );

        let tools = raw_tools
            .into_iter()
            .map(|(_, tool)| tool)
            .map(|(name, raw)| {
                let aliases = raw.mcp_aliases.clone();
                let tool = ToolContract::from_raw(name, raw)?;
                let mut tools = Vec::with_capacity(aliases.len() + 1);
                tools.push(tool.clone());
                tools.extend(
                    aliases
                        .into_iter()
                        .map(|alias| tool.alias(alias))
                        .collect::<Result<Vec<_>, _>>()?,
                );
                Ok(tools)
            })
            .collect::<Result<Vec<_>, String>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        validate_unique_render_names(&tools)?;

        Ok(Self {
            shared: parsed.shared.unwrap_or_default(),
            skill: parsed.skill,
            tools,
        })
    }

    pub fn shared(&self) -> &SharedContent {
        &self.shared
    }

    pub fn skill(&self) -> Option<&SkillConfig> {
        self.skill.as_ref()
    }

    pub fn tools(&self) -> &[ToolContract] {
        &self.tools
    }

    pub fn tool_list_value(&self) -> Value {
        let tools = self
            .tools
            .iter()
            .map(|tool| tool.tool_entry_value(&self.shared))
            .collect::<Vec<_>>();
        json!({ "tools": tools })
    }
}

const TOOL_CONTRACT_ORDER: &[&str] = &[
    "session_run",
    "session_list",
    "session_get",
    "namespace_list",
    "namespace_get",
    "session_capture",
    "session_delete",
    "session_label",
    "mail_send",
    "mail_read",
    "mail_check",
    "mail_stop_check",
    "nudge",
    "logs",
    "wait",
    "doctor",
];

fn tool_contract_sort_key(name: &str, original_index: usize) -> (usize, usize) {
    let index = TOOL_CONTRACT_ORDER
        .iter()
        .position(|known| *known == name)
        .unwrap_or(TOOL_CONTRACT_ORDER.len());
    (index, original_index)
}

fn validate_unique_render_names(tools: &[ToolContract]) -> Result<(), String> {
    let mut tool_names = HashSet::new();
    let mut const_names = HashSet::new();
    for tool in tools {
        if !tool_names.insert(tool.name.as_str()) {
            return Err(format!("duplicate MCP tool name {}", tool.name));
        }
        if !tool.artifacts.render_cli_help {
            continue;
        }
        let prefix = &tool.artifacts.cli_help_prefix;
        let about_const = format!("{prefix}_ABOUT");
        if !const_names.insert(about_const.clone()) {
            return Err(format!("duplicate CLI help constant {about_const}"));
        }
        for param in &tool.params {
            if param.cli_help.is_none() {
                continue;
            }
            let const_name = format!("{prefix}_{}_HELP", rust_const_name(&param.name));
            if !const_names.insert(const_name.clone()) {
                return Err(format!("duplicate CLI help constant {const_name}"));
            }
        }
    }
    Ok(())
}
