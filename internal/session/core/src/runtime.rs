use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{SmError, SmResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Claude,
    Codex,
}

impl RuntimeKind {
    pub fn command(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

impl fmt::Display for RuntimeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.command())
    }
}

impl FromStr for RuntimeKind {
    type Err = SmError;

    fn from_str(value: &str) -> SmResult<Self> {
        match value {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            other => Err(SmError::UnsupportedRuntime(other.to_string())),
        }
    }
}
