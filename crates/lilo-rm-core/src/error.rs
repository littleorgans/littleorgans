use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    RuntimeUnavailable,
    SessionNotFound,
    TmuxPaneDead,
    HeadlessNudgeUnsupported,
    LaunchFailed,
    InvalidTarget,
    DockerImageNotConfigured,
    UnsupportedIsolationPolicy,
    SpawnConflict,
    ProtocolMismatch,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::SessionNotFound => "session_not_found",
            Self::TmuxPaneDead => "tmux_pane_dead",
            Self::HeadlessNudgeUnsupported => "headless_nudge_unsupported",
            Self::LaunchFailed => "launch_failed",
            Self::InvalidTarget => "invalid_target",
            Self::DockerImageNotConfigured => "docker_image_not_configured",
            Self::UnsupportedIsolationPolicy => "unsupported_isolation_policy",
            Self::SpawnConflict => "spawn_conflict",
            Self::ProtocolMismatch => "protocol_mismatch",
        }
    }
}

impl Display for ErrorCode {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("connection closed before a message arrived")]
    Eof,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported runtime protocol version: expected {expected}, got {got}")]
    UnsupportedVersion { expected: &'static str, got: String },
}

#[derive(Debug, Error)]
#[error("unsupported runtime kind: {0}")]
pub struct RuntimeKindParseError(pub String);
