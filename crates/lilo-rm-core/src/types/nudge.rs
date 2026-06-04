use lilo_common::id::SessionId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NudgeRequest {
    pub session_id: SessionId,
    pub content: String,
    pub mode: NudgeMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// Delivery policy for a nudge, expressing how to treat a recipient that is
/// mid-work. `Immediate` is the historical fire-now behaviour; `Wait` and
/// `Steer` are requested by `lilo mail send --notify`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NudgeMode {
    /// Deliver right away regardless of recipient state (bare `lilo nudge`,
    /// MCP nudge, diagnostic `lilo runtime nudge`).
    Immediate,
    /// Wait until the recipient agent is idle, then deliver. Times out to
    /// `AgentBusyTimeout` without delivering.
    Wait,
    /// If the recipient agent is working, interrupt it first, then deliver.
    Steer,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NudgeResponse {
    pub delivered: bool,
    pub outcome: NudgeOutcome,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum NudgeOutcome {
    Delivered,
    Unsupported(NudgeFailureReason),
    Failed(NudgeFailureReason),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NudgeFailureReason {
    HeadlessLifecycle,
    SessionEnded,
    TmuxPaneDead,
    AgentBusyTimeout,
}

impl NudgeFailureReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HeadlessLifecycle => "headless_lifecycle",
            Self::SessionEnded => "session_ended",
            Self::TmuxPaneDead => "tmux_pane_dead",
            Self::AgentBusyTimeout => "agent_busy_timeout",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nudge_outcome_reason_strings_match_public_contract() {
        assert_eq!(
            NudgeFailureReason::HeadlessLifecycle.as_str(),
            "headless_lifecycle"
        );
        assert_eq!(NudgeFailureReason::SessionEnded.as_str(), "session_ended");
        assert_eq!(NudgeFailureReason::TmuxPaneDead.as_str(), "tmux_pane_dead");
        assert_eq!(
            NudgeFailureReason::AgentBusyTimeout.as_str(),
            "agent_busy_timeout"
        );
    }
}
