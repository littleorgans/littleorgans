use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum LogAvailability {
    Headless {
        stdout_path: PathBuf,
        stderr_path: PathBuf,
    },
    TmuxPaneSnapshot,
    Unavailable {
        reason: LogsUnavailableReason,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogsUnavailableReason {
    TmuxTarget,
    CaptureDisabled,
    PaneUnavailable,
    PipeInUse,
    RecorderFailed,
}

impl LogsUnavailableReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TmuxTarget => "tmux_target",
            Self::CaptureDisabled => "capture_disabled",
            Self::PaneUnavailable => "pane_unavailable",
            Self::PipeInUse => "pipe_in_use",
            Self::RecorderFailed => "recorder_failed",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CaptureRequest {
    pub session_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrollback_lines: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PaneSnapshot {
    pub content: String,
    pub captured_at_ms: u64,
    pub scrollback_lines_requested: u32,
    pub scrollback_lines_included: u32,
    pub pane_history_lines: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, thiserror::Error)]
#[non_exhaustive]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CaptureError {
    #[error("session is not attached to a tmux target")]
    NotATmuxTarget,
    #[error("tmux pane is unavailable")]
    PaneUnavailable,
    #[error("session not found")]
    SessionMissing,
    #[error("tmux is not available")]
    TmuxNotAvailable,
    #[error("tmux capture-pane failed: {stderr}")]
    CapturePaneFailed { stderr: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(tag = "status", content = "payload", rename_all = "snake_case")]
pub enum CaptureResponse {
    Captured(PaneSnapshot),
    Failed(CaptureError),
}

impl CaptureResponse {
    pub fn into_result(self) -> Result<PaneSnapshot, CaptureError> {
        match self {
            Self::Captured(snapshot) => Ok(snapshot),
            Self::Failed(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LogsUnavailableReason;

    #[test]
    fn logs_unavailable_reason_as_str_matches_serde_snake_case() {
        assert_eq!(LogsUnavailableReason::TmuxTarget.as_str(), "tmux_target");
        assert_eq!(
            LogsUnavailableReason::CaptureDisabled.as_str(),
            "capture_disabled"
        );
        assert_eq!(
            LogsUnavailableReason::PaneUnavailable.as_str(),
            "pane_unavailable"
        );
        assert_eq!(LogsUnavailableReason::PipeInUse.as_str(), "pipe_in_use");
        assert_eq!(
            LogsUnavailableReason::RecorderFailed.as_str(),
            "recorder_failed"
        );
    }
}
