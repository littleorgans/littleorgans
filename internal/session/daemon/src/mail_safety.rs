use std::fs;

use anyhow::{Context, Result, bail};
use chrono::Duration;
use lilo_paths::{LiloHome, LiloPaths};
use serde::Deserialize;

const DEFAULT_CONVERSATION_DEPTH_LIMIT: usize = 50;
const DEFAULT_SENDER_RATE_LIMIT: usize = 30;
const DEFAULT_SENDER_RATE_WINDOW_SECS: i64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MailSafetyConfig {
    pub conversation_depth_limit: usize,
    pub sender_rate_limit: usize,
    pub sender_rate_window: Duration,
}

impl MailSafetyConfig {
    pub(crate) fn from_limits(
        conversation_depth_limit: usize,
        sender_rate_limit: usize,
        sender_rate_window_secs: i64,
    ) -> Self {
        Self {
            conversation_depth_limit,
            sender_rate_limit,
            sender_rate_window: Duration::seconds(sender_rate_window_secs),
        }
    }

    pub(crate) fn load() -> Self {
        match Self::load_from_env() {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(error = ?error, "mail safety config failed; using defaults");
                Self::default()
            }
        }
    }

    fn load_from_env() -> Result<Self> {
        let paths = LiloPaths::new(LiloHome::from_env()?);
        let path = paths.config_root().join("session").join("mail.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read mail safety config {}", path.display()))?;
        let config: MailSafetyToml = toml::from_str(&content)
            .with_context(|| format!("failed to parse mail safety config {}", path.display()))?;
        config.safety.into_config()
    }
}

impl Default for MailSafetyConfig {
    fn default() -> Self {
        Self {
            conversation_depth_limit: DEFAULT_CONVERSATION_DEPTH_LIMIT,
            sender_rate_limit: DEFAULT_SENDER_RATE_LIMIT,
            sender_rate_window: Duration::seconds(DEFAULT_SENDER_RATE_WINDOW_SECS),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct MailSafetyToml {
    #[serde(default)]
    safety: MailSafetySection,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct MailSafetySection {
    conversation_depth_limit: Option<usize>,
    sender_rate_limit: Option<usize>,
    sender_rate_window_secs: Option<i64>,
}

impl MailSafetySection {
    fn into_config(self) -> Result<MailSafetyConfig> {
        let conversation_depth_limit = nonzero_usize(
            self.conversation_depth_limit,
            DEFAULT_CONVERSATION_DEPTH_LIMIT,
            "safety.conversation_depth_limit",
        )?;
        let sender_rate_limit = nonzero_usize(
            self.sender_rate_limit,
            DEFAULT_SENDER_RATE_LIMIT,
            "safety.sender_rate_limit",
        )?;
        let sender_rate_window_secs = nonzero_i64(
            self.sender_rate_window_secs,
            DEFAULT_SENDER_RATE_WINDOW_SECS,
            "safety.sender_rate_window_secs",
        )?;
        Ok(MailSafetyConfig {
            conversation_depth_limit,
            sender_rate_limit,
            sender_rate_window: Duration::seconds(sender_rate_window_secs),
        })
    }
}

fn nonzero_usize(value: Option<usize>, default: usize, field: &'static str) -> Result<usize> {
    match value {
        Some(0) => bail!("{field} must be greater than zero"),
        Some(value) => Ok(value),
        None => Ok(default),
    }
}

fn nonzero_i64(value: Option<i64>, default: i64, field: &'static str) -> Result<i64> {
    match value {
        Some(value) if value <= 0 => bail!("{field} must be greater than zero"),
        Some(value) => Ok(value),
        None => Ok(default),
    }
}
