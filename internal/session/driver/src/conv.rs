use std::path::PathBuf;
use std::str::FromStr;

use lilo_rm_core::{
    KillOutcome, Lifecycle, LifecycleState, RuntimeKind as RuntimeRuntimeKind,
    SpawnRequest as RuntimeSpawnRequest, SpawnTarget as RuntimeSpawnTarget, SpawnedPayload,
};
use lilo_session_core::{
    RuntimeKind, paths::lifecycle_transcript_path as session_lifecycle_transcript_path,
};
use uuid::Uuid;

use crate::driver::{DriverError, DriverProbe, SpawnLaunch, SpawnedProcess};

pub fn runtime_spawn_request(
    session_id: Uuid,
    launch: &SpawnLaunch,
) -> Result<RuntimeSpawnRequest, DriverError> {
    Ok(RuntimeSpawnRequest {
        session_id,
        runtime: runtime_kind(launch.runtime),
        isolation: launch.isolation.clone(),
        image: launch.image.clone(),
        env: launch.env.clone(),
        mounts: launch.mounts.clone(),
        cwd: launch.cwd.clone(),
        target: RuntimeSpawnTarget::from_str(&launch.target)
            .map_err(|_| DriverError::InvalidTarget(launch.target.clone()))?,
        force: launch.force,
        shell_resume: launch.shell_resume.clone(),
    })
}

pub fn spawned_process(payload: SpawnedPayload) -> Result<SpawnedProcess, DriverError> {
    Ok(SpawnedProcess {
        runtime_pid: runtime_pid(&payload.lifecycle)?,
        log_dir: payload.log_dir,
        stdout_path: payload.stdout_path,
        stderr_path: payload.stderr_path,
        tmux_pane: payload.lifecycle.tmux_pane.map(|pane| pane.to_string()),
    })
}

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

pub(crate) fn lifecycle_transcript_path(lifecycle: &Lifecycle) -> Option<PathBuf> {
    session_lifecycle_transcript_path(lifecycle)
}

fn runtime_kind(runtime: RuntimeKind) -> RuntimeRuntimeKind {
    match runtime {
        RuntimeKind::Claude => RuntimeRuntimeKind::Claude,
        RuntimeKind::Codex => RuntimeRuntimeKind::Codex,
    }
}

fn runtime_pid(lifecycle: &Lifecycle) -> Result<u32, DriverError> {
    lifecycle
        .runtime_pid
        .ok_or_else(|| DriverError::MissingRuntimePid(lifecycle.session_id.to_string()))
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
