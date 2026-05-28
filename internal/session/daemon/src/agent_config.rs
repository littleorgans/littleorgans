use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use lilo_paths::{LiloHome, LiloPaths};
use lilo_session_core::is_agent_config_path_like;
use lilo_session_driver::LaunchEnv;
use serde::Deserialize;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, Visitor};
use toml::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAgentConfig {
    pub requested: String,
    pub path: PathBuf,
    pub env: Vec<LaunchEnv>,
}

pub fn resolve_agent_config(requested: Option<&str>) -> Result<Option<ResolvedAgentConfig>> {
    let Some(requested) = requested else {
        return Ok(None);
    };
    let path = agent_config_path_from_env(requested)?;
    resolve_agent_config_at_path(requested, path).map(Some)
}

fn resolve_agent_config_at_path(requested: &str, path: PathBuf) -> Result<ResolvedAgentConfig> {
    if !path.is_file() {
        bail!(
            "agent config not found: {requested} (looked for {})",
            path.display()
        );
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read agent config {}", path.display()))?;
    let value = content
        .parse::<Value>()
        .with_context(|| format!("failed to parse agent config {}", path.display()))?;
    let config = AgentConfigToml::deserialize(value)?;
    let env = agent_env(config);

    Ok(ResolvedAgentConfig {
        requested: requested.to_string(),
        path,
        env,
    })
}

fn agent_config_path_from_env(requested: &str) -> Result<PathBuf> {
    if is_agent_config_path_like(requested) {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        return expand_home(requested, home.as_deref());
    }

    let home = LiloHome::from_env().context("failed to resolve LILO_HOME for agent config")?;
    Ok(named_agent_config_path(requested, &LiloPaths::new(home)))
}

fn named_agent_config_path(requested: &str, paths: &LiloPaths) -> PathBuf {
    paths.agent_config_dir(requested).join("agent.toml")
}

fn expand_home(value: &str, home: Option<&Path>) -> Result<PathBuf> {
    if value == "~" {
        return home
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("HOME is required to expand agent config path {value}"));
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return home
            .map(|home| home.join(rest))
            .ok_or_else(|| anyhow!("HOME is required to expand agent config path {value}"));
    }
    Ok(PathBuf::from(value))
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentConfigToml {
    #[serde(default, deserialize_with = "deserialize_claude_config_dir")]
    claude_config_dir: Option<String>,
    #[serde(default)]
    env: AgentConfigEnv,
}

#[derive(Debug, Default)]
struct AgentConfigEnv(BTreeMap<String, String>);

impl<'de> Deserialize<'de> for AgentConfigEnv {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(AgentConfigEnvVisitor)
    }
}

struct AgentConfigEnvVisitor;

impl<'de> Visitor<'de> for AgentConfigEnvVisitor {
    type Value = AgentConfigEnv;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("agent config `env` must be a table")
    }

    fn visit_map<M>(self, mut access: M) -> std::result::Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut env = BTreeMap::new();
        while let Some(key) = access.next_key::<String>()? {
            let value = access.next_value_seed(AgentConfigEnvValue { key: &key })?;
            env.insert(key, value);
        }
        Ok(AgentConfigEnv(env))
    }
}

struct AgentConfigEnvValue<'a> {
    key: &'a str,
}

impl<'de> DeserializeSeed<'de> for AgentConfigEnvValue<'_> {
    type Value = String;

    fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map_err(|_| {
            de::Error::custom(format!("agent config env `{}` must be a string", self.key))
        })
    }
}

fn deserialize_claude_config_dir<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer)
        .map(Some)
        .map_err(|_| de::Error::custom("agent config `claude_config_dir` must be a string"))
}

fn agent_env(config: AgentConfigToml) -> Vec<LaunchEnv> {
    let mut env = BTreeMap::new();
    if let Some(path) = config.claude_config_dir {
        env.insert("CLAUDE_CONFIG_DIR".to_string(), path);
    }
    env.extend(config.env.0);
    env.into_iter()
        .map(|(key, value)| LaunchEnv { key, value })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ErrOrPanic as _, OrPanic as _};

    #[test]
    fn resolves_named_agent_config_from_lilo_home() {
        let dir = tempfile::tempdir().or_panic("tempdir creates");
        let paths = lilo_paths(dir.path().join("lilo"));
        let config_dir = paths.agent_config_dir("demo-agent");
        fs::create_dir_all(&config_dir).or_panic("config dir creates");
        fs::write(
            config_dir.join("agent.toml"),
            "claude_config_dir = \"/tmp/claude\"\n[env]\nHELIOY_AGENT_NAME = \"demo\"\n",
        )
        .or_panic("config writes");

        let resolved = resolve_agent_config_at_path(
            "demo-agent",
            named_agent_config_path("demo-agent", &paths),
        )
        .or_panic("config resolves");

        assert_eq!(resolved.requested, "demo-agent");
        assert_eq!(resolved.path, config_dir.join("agent.toml"));
        assert_eq!(
            resolved.env,
            vec![
                LaunchEnv {
                    key: "CLAUDE_CONFIG_DIR".to_string(),
                    value: "/tmp/claude".to_string(),
                },
                LaunchEnv {
                    key: "HELIOY_AGENT_NAME".to_string(),
                    value: "demo".to_string(),
                },
            ]
        );
    }

    #[test]
    fn bare_toml_filename_resolves_as_lilo_named_config() {
        let dir = tempfile::tempdir().or_panic("tempdir creates");
        let paths = lilo_paths(dir.path().join("lilo"));
        let config_dir = paths.agent_config_dir("tools.toml");
        fs::create_dir_all(&config_dir).or_panic("config dir creates");
        fs::write(
            config_dir.join("agent.toml"),
            "[env]\nHELIOY_AGENT_NAME = \"tools\"\n",
        )
        .or_panic("config writes");

        let resolved = resolve_agent_config_at_path(
            "tools.toml",
            named_agent_config_path("tools.toml", &paths),
        )
        .or_panic("config resolves");

        assert_eq!(resolved.path, config_dir.join("agent.toml"));
    }

    #[test]
    fn legacy_agm_named_config_is_ignored() {
        let dir = tempfile::tempdir().or_panic("tempdir creates");
        let paths = lilo_paths(dir.path().join("lilo"));
        let legacy_config_dir = dir.path().join(".agm/demo-agent");
        fs::create_dir_all(&legacy_config_dir).or_panic("legacy config dir creates");
        fs::write(
            legacy_config_dir.join("agent.toml"),
            "[env]\nHELIOY_AGENT_NAME = \"legacy\"\n",
        )
        .or_panic("legacy config writes");

        let error = resolve_agent_config_at_path(
            "demo-agent",
            named_agent_config_path("demo-agent", &paths),
        )
        .err_or_panic("legacy config is ignored");

        assert!(error.to_string().contains("agent config not found"));
        assert!(
            error
                .to_string()
                .contains(&paths.agent_config_dir("demo-agent").display().to_string())
        );
    }

    #[test]
    fn resolves_explicit_agent_config_path() {
        let dir = tempfile::tempdir().or_panic("tempdir creates");
        let path = dir.path().join("agent.toml");
        fs::write(&path, "[env]\nHELIOY_AGENT_NAME = \"explicit\"\n").or_panic("config writes");

        let requested = path.to_str().or_panic("path is utf8").to_string();
        let resolved =
            resolve_agent_config_at_path(&requested, path.clone()).or_panic("config resolves");

        assert_eq!(resolved.requested, path.to_string_lossy());
        assert_eq!(
            resolved.env,
            vec![LaunchEnv {
                key: "HELIOY_AGENT_NAME".to_string(),
                value: "explicit".to_string(),
            }]
        );
    }

    #[test]
    fn env_table_claude_config_dir_overrides_top_level_value() {
        let resolved = resolve_inline_config(
            "claude_config_dir = \"/a\"\n[env]\nCLAUDE_CONFIG_DIR = \"/b\"\n",
        )
        .or_panic("config resolves");

        assert_eq!(
            resolved.env,
            vec![LaunchEnv {
                key: "CLAUDE_CONFIG_DIR".to_string(),
                value: "/b".to_string(),
            }]
        );
    }

    #[test]
    fn unknown_top_level_agent_config_key_is_rejected() {
        let error =
            resolve_inline_config("clade_config_dir = \"/x\"\n").err_or_panic("unknown key fails");
        let message = format!("{error:#}");

        assert!(message.contains("unknown field `clade_config_dir`"));
    }

    #[test]
    fn non_string_agent_config_env_value_names_key() {
        let error = resolve_inline_config("[env]\nKEY = 42\n").err_or_panic("non-string env fails");
        let message = format!("{error:#}");

        assert!(message.contains("agent config env"));
        assert!(message.contains("KEY"));
    }

    #[test]
    fn missing_agent_config_is_structured_error() {
        let dir = tempfile::tempdir().or_panic("tempdir creates");
        let paths = lilo_paths(dir.path().join("lilo"));
        let error = resolve_agent_config_at_path(
            "missing-agent",
            named_agent_config_path("missing-agent", &paths),
        )
        .err_or_panic("missing config fails");

        assert!(error.to_string().contains("agent config not found"));
        assert!(error.to_string().contains("missing-agent"));
    }

    fn resolve_inline_config(content: &str) -> Result<ResolvedAgentConfig> {
        let dir = tempfile::tempdir().or_panic("tempdir creates");
        let path = dir.path().join("agent.toml");
        fs::write(&path, content).or_panic("config writes");

        let requested = path.to_str().or_panic("path is utf8").to_string();
        resolve_agent_config_at_path(&requested, path)
    }

    fn lilo_paths(root: PathBuf) -> LiloPaths {
        LiloPaths::new(LiloHome::from_path(root).or_panic("lilo home"))
    }
}
