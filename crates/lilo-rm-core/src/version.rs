use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::string_serde::{deserialize_string_parsed, serialize_string};

pub const RUNTIME_PROTOCOL_VERSION: &str = "0.6";

pub const RUNTIME_PROTOCOL_CAPABILITIES: &[RuntimeCapability] = &[
    RuntimeCapability::StructuredProtocolErrors,
    RuntimeCapability::HeadlessStdioLogPaths,
    RuntimeCapability::StatusSessionSetFilter,
    RuntimeCapability::StatusUpdatedSinceFilter,
    RuntimeCapability::TypedNudgeOutcomes,
    RuntimeCapability::ValidateTargetPreflight,
    RuntimeCapability::EventsCursor,
    RuntimeCapability::EventsLongPoll,
    RuntimeCapability::TmuxPaneSnapshot,
    RuntimeCapability::KillOutcomes,
    RuntimeCapability::SpawnConflicts,
    RuntimeCapability::SpawnRequestMounts,
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VersionInfo {
    pub version: String,
    pub git_sha: String,
    pub protocol_version: String,
    pub capabilities: Vec<RuntimeCapability>,
}

impl VersionInfo {
    pub fn new(version: impl Into<String>, git_sha: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            git_sha: git_sha.into(),
            protocol_version: RUNTIME_PROTOCOL_VERSION.to_owned(),
            capabilities: RUNTIME_PROTOCOL_CAPABILITIES.to_vec(),
        }
    }
}

pub fn version_info() -> VersionInfo {
    VersionInfo::new(env!("CARGO_PKG_VERSION"), env!("RTM_GIT_SHA"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RuntimeCapability {
    /// Error responses expose stable machine readable codes.
    StructuredProtocolErrors,
    /// Headless spawn responses include stdout and stderr log paths.
    HeadlessStdioLogPaths,
    /// Status requests accept a set of session ids.
    StatusSessionSetFilter,
    /// Status requests accept an updated time lower bound.
    StatusUpdatedSinceFilter,
    /// Nudge responses expose typed delivery outcomes.
    TypedNudgeOutcomes,
    /// `ValidateTarget` checks a target string without spawning.
    ValidateTargetPreflight,
    /// Events support durable cursor replay.
    EventsCursor,
    /// Events requests accept a bounded long poll wait window.
    EventsLongPoll,
    /// Tmux targets support on demand pane snapshot capture.
    TmuxPaneSnapshot,
    /// Kill responses include typed `Signalled` or `AlreadyExited` outcomes.
    KillOutcomes,
    /// Spawn rejects session reuse and occupied tmux panes with typed conflicts.
    SpawnConflicts,
    /// Spawn requests can carry declared host to container bind mounts.
    SpawnRequestMounts,
}

impl RuntimeCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StructuredProtocolErrors => "structured_protocol_errors",
            Self::HeadlessStdioLogPaths => "headless_stdio_log_paths",
            Self::StatusSessionSetFilter => "status_session_set_filter",
            Self::StatusUpdatedSinceFilter => "status_updated_since_filter",
            Self::TypedNudgeOutcomes => "typed_nudge_outcomes",
            Self::ValidateTargetPreflight => "validate_target_preflight",
            Self::EventsCursor => "events_cursor",
            Self::EventsLongPoll => "events_long_poll",
            Self::TmuxPaneSnapshot => "tmux_pane_snapshot",
            Self::KillOutcomes => "kill_outcomes",
            Self::SpawnConflicts => "spawn_conflicts",
            Self::SpawnRequestMounts => "spawn_request_mounts",
        }
    }
}

impl Display for RuntimeCapability {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RuntimeCapability {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "structured_protocol_errors" => Ok(Self::StructuredProtocolErrors),
            "headless_stdio_log_paths" => Ok(Self::HeadlessStdioLogPaths),
            "status_session_set_filter" => Ok(Self::StatusSessionSetFilter),
            "status_updated_since_filter" => Ok(Self::StatusUpdatedSinceFilter),
            "typed_nudge_outcomes" => Ok(Self::TypedNudgeOutcomes),
            "validate_target_preflight" => Ok(Self::ValidateTargetPreflight),
            "events_cursor" => Ok(Self::EventsCursor),
            "events_long_poll" => Ok(Self::EventsLongPoll),
            "tmux_pane_snapshot" => Ok(Self::TmuxPaneSnapshot),
            "kill_outcomes" => Ok(Self::KillOutcomes),
            "spawn_conflicts" => Ok(Self::SpawnConflicts),
            "spawn_request_mounts" => Ok(Self::SpawnRequestMounts),
            other => Err(format!("unknown runtime capability {other}")),
        }
    }
}

impl Serialize for RuntimeCapability {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_string(self.as_str(), serializer)
    }
}

impl<'de> Deserialize<'de> for RuntimeCapability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_string_parsed(deserializer)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RUNTIME_PROTOCOL_CAPABILITIES, RUNTIME_PROTOCOL_VERSION, RuntimeCapability, VersionInfo,
    };

    #[test]
    fn protocol_version_advertises_v06_spawn_conflict_contract() {
        assert_eq!(RUNTIME_PROTOCOL_VERSION, "0.6");
        assert_eq!(VersionInfo::new("rtm", "git").protocol_version, "0.6");
    }

    #[test]
    fn protocol_capabilities_advertise_spawn_request_mounts() {
        assert!(RUNTIME_PROTOCOL_CAPABILITIES.contains(&RuntimeCapability::SpawnRequestMounts));
        assert_eq!(
            RuntimeCapability::SpawnRequestMounts.as_str(),
            "spawn_request_mounts"
        );
    }
}
