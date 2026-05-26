use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::RuntimeKindParseError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeKind {
    Claude,
    Codex,
    Other(String),
}

impl RuntimeKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Other(value) => value,
        }
    }
}

impl Display for RuntimeKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RuntimeKind {
    type Err = RuntimeKindParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() {
            return Err(RuntimeKindParseError(value.to_owned()));
        }

        match value {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            other => Ok(Self::Other(other.to_owned())),
        }
    }
}

impl Serialize for RuntimeKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RuntimeKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSignal {
    Hup,
    Int,
    Term,
    Kill,
}

impl RuntimeSignal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hup => "HUP",
            Self::Int => "INT",
            Self::Term => "TERM",
            Self::Kill => "KILL",
        }
    }
}

impl Display for RuntimeSignal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RuntimeSignal {
    type Err = RuntimeSignalParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value
            .trim_start_matches("SIG")
            .to_ascii_uppercase()
            .as_str()
        {
            "HUP" => Ok(Self::Hup),
            "INT" => Ok(Self::Int),
            "TERM" => Ok(Self::Term),
            "KILL" => Ok(Self::Kill),
            other => Err(RuntimeSignalParseError(other.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("unsupported signal {0}")]
pub struct RuntimeSignalParseError(pub String);
