use std::path::PathBuf;

use lilo_rm_core::{KillOutcome, Lifecycle, LifecycleState, LogAvailability};

use crate::driver::{DriverError, DriverProbe};

pub(crate) fn lifecycle_to_probe(
    lifecycle: &Lifecycle,
    expected_runtime_pid: u32,
) -> Result<DriverProbe, DriverError> {
    let Some(runtime_pid) = lifecycle.runtime_pid else {
        return Ok(DriverProbe {
            verified: false,
            evidence: format!(
                "runtime session {} has no runtime pid",
                lifecycle.session_id
            ),
            transcript_path: lifecycle_transcript_path(lifecycle),
        });
    };

    if runtime_pid != expected_runtime_pid {
        return Ok(DriverProbe {
            verified: false,
            evidence: format!(
                "stored runtime pid {expected_runtime_pid} does not match rtmd pid {runtime_pid}"
            ),
            transcript_path: lifecycle_transcript_path(lifecycle),
        });
    }

    match lifecycle.state {
        LifecycleState::Forking | LifecycleState::Running => Ok(DriverProbe {
            verified: true,
            evidence: "rtmd lifecycle is active".to_string(),
            transcript_path: lifecycle_transcript_path(lifecycle),
        }),
        LifecycleState::Exited(exit) => Ok(DriverProbe {
            verified: false,
            evidence: format!("rtmd lifecycle exited: {exit}"),
            transcript_path: lifecycle_transcript_path(lifecycle),
        }),
        LifecycleState::Lost(evidence) => Ok(DriverProbe {
            verified: false,
            evidence: format!("rtmd lifecycle lost: {evidence}"),
            transcript_path: lifecycle_transcript_path(lifecycle),
        }),
        _ => Err(DriverError::UnknownRuntimeVariant {
            variant: lifecycle_state_label(&lifecycle.state),
        }),
    }
}

pub(crate) fn lifecycle_transcript_path(lifecycle: &Lifecycle) -> Option<PathBuf> {
    match lifecycle.log_availability.as_ref() {
        Some(LogAvailability::Headless { stdout_path, .. }) => Some(stdout_path.clone()),
        Some(LogAvailability::TmuxPaneSnapshot | LogAvailability::Unavailable { .. }) | None => {
            None
        }
    }
}

pub(crate) fn lifecycle_state_label(state: &LifecycleState) -> String {
    match state {
        LifecycleState::Forking => "forking".to_string(),
        LifecycleState::Running => "running".to_string(),
        LifecycleState::Exited(_) => "exited".to_string(),
        LifecycleState::Lost(_) => "lost".to_string(),
        other => format!("unknown ({other:?})"),
    }
}

pub(crate) fn kill_outcome_label(outcome: KillOutcome) -> String {
    match outcome {
        KillOutcome::Signalled => "signalled".to_string(),
        KillOutcome::AlreadyExited => "already_exited".to_string(),
        other => format!("unknown ({other:?})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilo_rm_core::{LostEvidence, RuntimeExit};

    #[test]
    fn lifecycle_state_label_covers_known_variants() {
        assert_eq!(lifecycle_state_label(&LifecycleState::Forking), "forking");
        assert_eq!(lifecycle_state_label(&LifecycleState::Running), "running");
        assert_eq!(
            lifecycle_state_label(&LifecycleState::Exited(RuntimeExit::new(Some(0), None))),
            "exited"
        );
        assert_eq!(
            lifecycle_state_label(&LifecycleState::Lost(LostEvidence::ShimDiedBeforeReport)),
            "lost"
        );
    }

    #[test]
    fn kill_outcome_label_covers_known_variants() {
        assert_eq!(kill_outcome_label(KillOutcome::Signalled), "signalled");
        assert_eq!(
            kill_outcome_label(KillOutcome::AlreadyExited),
            "already_exited"
        );
    }
}
