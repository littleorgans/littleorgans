use serde::{Deserialize, Serialize};

use super::{SpawnTargetParseError, TmuxAddress};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ValidateTargetRequest {
    pub target: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ValidateTargetResponse {
    pub valid: bool,
    pub outcome: ValidateTargetOutcome,
}

impl ValidateTargetResponse {
    pub fn valid() -> Self {
        Self {
            valid: true,
            outcome: ValidateTargetOutcome::Valid,
        }
    }

    pub fn invalid_target(error: &SpawnTargetParseError) -> Self {
        Self {
            valid: false,
            outcome: ValidateTargetOutcome::InvalidTarget {
                message: error.to_string(),
            },
        }
    }

    pub fn tmux_pane_dead(address: TmuxAddress) -> Self {
        Self {
            valid: false,
            outcome: ValidateTargetOutcome::TmuxPaneDead { address },
        }
    }

    pub fn unsupported_target(target: impl Into<String>) -> Self {
        Self {
            valid: false,
            outcome: ValidateTargetOutcome::UnsupportedTarget {
                target: target.into(),
            },
        }
    }

    pub fn from_target_parse_error(error: SpawnTargetParseError) -> Self {
        if is_unsupported_target(&error.0) {
            Self::unsupported_target(error.0)
        } else {
            Self::invalid_target(&error)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ValidateTargetOutcome {
    Valid,
    InvalidTarget { message: String },
    TmuxPaneDead { address: TmuxAddress },
    UnsupportedTarget { target: String },
}

fn is_unsupported_target(target: &str) -> bool {
    target
        .split_once(':')
        .is_some_and(|(mode, _)| !mode.is_empty() && mode != "tmux")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SpawnTarget;

    #[test]
    fn validate_target_parse_errors_are_typed() {
        let invalid = ValidateTargetResponse::from_target_parse_error(
            "tmux:not-a-pane"
                .parse::<SpawnTarget>()
                .expect_err("invalid tmux target"),
        );
        assert!(!invalid.valid);
        assert!(matches!(
            invalid.outcome,
            ValidateTargetOutcome::InvalidTarget { .. }
        ));

        assert_eq!(
            ValidateTargetResponse::from_target_parse_error(
                "ssh:remote"
                    .parse::<SpawnTarget>()
                    .expect_err("unsupported target"),
            ),
            ValidateTargetResponse::unsupported_target("ssh:remote")
        );
    }
}
