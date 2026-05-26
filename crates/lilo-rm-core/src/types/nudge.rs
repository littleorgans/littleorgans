use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NudgeRequest {
    pub session_id: Uuid,
    pub content: String,
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
}

impl NudgeFailureReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HeadlessLifecycle => "headless_lifecycle",
            Self::SessionEnded => "session_ended",
            Self::TmuxPaneDead => "tmux_pane_dead",
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
    }
}
