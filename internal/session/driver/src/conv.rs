use std::path::PathBuf;
use std::str::FromStr;

use lilo_rm_core::{
    CaptureResponse, DoctorResponse as RuntimeDoctorResponse, KillOutcome, Lifecycle,
    LifecycleState, NudgeFailureReason, NudgeOutcome, RUNTIME_PROTOCOL_VERSION,
    RuntimeKind as RuntimeRuntimeKind, RuntimeSignal, SpawnConflictKind, SpawnConflictPayload,
    SpawnRequest as RuntimeSpawnRequest, SpawnTarget as RuntimeSpawnTarget, SpawnedPayload,
    StatusFilter,
};
use lilo_runtime_daemon::SpawnOutcome;
use lilo_session_core::{
    RuntimeDoctorReport, RuntimeKind,
    paths::lifecycle_transcript_path as session_lifecycle_transcript_path,
};
use uuid::Uuid;

use crate::driver::{
    CaptureResult, ChildExit, DriverError, NudgeResult, SpawnLaunch, SpawnedProcess,
};

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
    let lifecycle = payload.lifecycle;
    let runtime_pid = runtime_pid(&lifecycle)?;
    let tmux_pane = lifecycle.tmux_pane.as_ref().map(ToString::to_string);
    Ok(SpawnedProcess {
        lifecycle,
        runtime_pid,
        log_dir: payload.log_dir,
        stdout_path: payload.stdout_path,
        stderr_path: payload.stderr_path,
        tmux_pane,
    })
}

pub(crate) fn spawn_outcome(outcome: SpawnOutcome) -> Result<SpawnedProcess, DriverError> {
    match outcome {
        SpawnOutcome::Spawned(payload) => spawned_process(payload),
        SpawnOutcome::Conflict(payload) => Err(spawn_conflict(&payload)),
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

pub(crate) fn status_session(session_id: Uuid) -> StatusFilter {
    StatusFilter::for_session(session_id)
}

pub(crate) fn terminal_child_exit(lifecycle: &Lifecycle) -> Result<Option<ChildExit>, DriverError> {
    let exit_code = match lifecycle.state {
        LifecycleState::Forking | LifecycleState::Running => return Ok(None),
        LifecycleState::Exited(exit) => exit.code,
        LifecycleState::Lost(_) => None,
        _ => {
            return Err(DriverError::UnknownRuntimeVariant {
                variant: lifecycle_state_label(&lifecycle.state),
            });
        }
    };
    Ok(Some(ChildExit {
        session_id: lifecycle.session_id.to_string(),
        runtime_pid: lifecycle.runtime_pid.unwrap_or_default(),
        exit_code,
        transcript_path: lifecycle_transcript_path(lifecycle),
    }))
}

pub(crate) fn capture_result(response: CaptureResponse) -> CaptureResult {
    CaptureResult { response }
}

pub(crate) fn nudge_result(outcome: &NudgeOutcome) -> NudgeResult {
    match outcome {
        NudgeOutcome::Delivered => NudgeResult {
            delivered: true,
            message: "delivered".to_string(),
        },
        NudgeOutcome::Unsupported(NudgeFailureReason::HeadlessLifecycle) => NudgeResult {
            delivered: false,
            message: "headless runtime does not support nudges".to_string(),
        },
        NudgeOutcome::Failed(NudgeFailureReason::SessionEnded) => NudgeResult {
            delivered: false,
            message: "session ended before the nudge could land".to_string(),
        },
        NudgeOutcome::Failed(NudgeFailureReason::TmuxPaneDead) => NudgeResult {
            delivered: false,
            message: "tmux pane is no longer available".to_string(),
        },
        NudgeOutcome::Unsupported(reason) => NudgeResult {
            delivered: false,
            message: format!("nudge unsupported ({})", reason.as_str()),
        },
        NudgeOutcome::Failed(reason) => NudgeResult {
            delivered: false,
            message: format!("nudge failed ({})", reason.as_str()),
        },
    }
}

pub(crate) fn runtime_doctor_report(
    doctor: RuntimeDoctorResponse,
    socket_path: Option<String>,
) -> RuntimeDoctorReport {
    let status = runtime_doctor_status(&doctor);
    RuntimeDoctorReport {
        status,
        doctor: Some(Box::new(doctor)),
        socket_path,
        code: None,
        message: None,
    }
}

pub(crate) fn runtime_doctor_error(
    code: Option<String>,
    message: String,
    socket_path: Option<String>,
) -> RuntimeDoctorReport {
    RuntimeDoctorReport {
        status: "error".to_string(),
        doctor: None,
        socket_path,
        code,
        message: Some(message),
    }
}

pub(crate) fn parse_session_id(session_id: &str) -> Result<Uuid, DriverError> {
    Uuid::parse_str(session_id).map_err(|_| DriverError::InvalidSessionId(session_id.to_string()))
}

pub(crate) fn parse_runtime_signal(signal: &str) -> Result<RuntimeSignal, DriverError> {
    RuntimeSignal::from_str(signal).map_err(|_| DriverError::InvalidSignal(signal.to_string()))
}

pub(crate) fn spawn_conflict(payload: &SpawnConflictPayload) -> DriverError {
    DriverError::SpawnConflict {
        kind: payload.kind,
        message: format_spawn_conflict(payload),
    }
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

fn runtime_doctor_status(doctor: &RuntimeDoctorResponse) -> String {
    if doctor.version.protocol_version != RUNTIME_PROTOCOL_VERSION
        || !doctor.sqlite.pending_descriptions.is_empty()
    {
        "degraded".to_string()
    } else {
        "ok".to_string()
    }
}

fn format_spawn_conflict(payload: &SpawnConflictPayload) -> String {
    let lifecycle = &payload.lifecycle;
    let runtime: &str = match &lifecycle.runtime {
        RuntimeRuntimeKind::Claude => "claude",
        RuntimeRuntimeKind::Codex => "codex",
        RuntimeRuntimeKind::Other(name) => name.as_str(),
    };
    let session_id = lifecycle.session_id;
    let pid = lifecycle
        .runtime_pid
        .map(|pid| format!(" (pid {pid})"))
        .unwrap_or_default();
    match payload.kind {
        SpawnConflictKind::TmuxPaneOccupancy => {
            let pane = lifecycle
                .tmux_pane
                .as_ref()
                .map_or_else(|| "<unknown>".to_string(), ToString::to_string);
            format!("tmux pane {pane} is already running {runtime} session {session_id}{pid}")
        }
        SpawnConflictKind::SessionId => {
            format!("session {session_id} is already running {runtime}{pid}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilo_rm_core::{
        CaptureError, DockerStatus, IsolationPolicy, LifecycleCounts, LostEvidence, MigrationState,
        RuntimeEvent, RuntimeExit, ShimReady, TmuxStatus, WatcherCounts, version_info,
    };

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

    #[test]
    fn nudge_result_maps_runtime_outcomes() {
        let delivered = nudge_result(&NudgeOutcome::Delivered);
        assert!(delivered.delivered);
        assert_eq!(delivered.message, "delivered");

        let headless = nudge_result(&NudgeOutcome::Unsupported(
            NudgeFailureReason::HeadlessLifecycle,
        ));
        assert!(!headless.delivered);
        assert_eq!(headless.message, "headless runtime does not support nudges");
    }

    #[test]
    fn capture_result_wraps_runtime_response() {
        let result = capture_result(CaptureResponse::Failed(CaptureError::NotATmuxTarget));
        assert_eq!(
            result.response,
            CaptureResponse::Failed(CaptureError::NotATmuxTarget)
        );
    }

    #[test]
    fn spawn_mappers_preserve_lifecycle_for_both_adapters() {
        let session_id = Uuid::now_v7();
        let start_time = chrono::Utc::now();
        let mut lifecycle = Lifecycle::forking(session_id, RuntimeRuntimeKind::Claude);
        lifecycle.isolation = IsolationPolicy::default();
        lifecycle.mark_running(ShimReady {
            session_id,
            shim_pid: 41,
            runtime_pid: 42,
            start_time,
            tmux_pane: None,
        });
        let payload = SpawnedPayload {
            lifecycle: lifecycle.clone(),
            event: RuntimeEvent::Running {
                session_id,
                runtime_pid: 42,
                start_time,
            },
            log_dir: Some(PathBuf::from("/tmp/logs")),
            stdout_path: Some(PathBuf::from("/tmp/logs/stdout.log")),
            stderr_path: Some(PathBuf::from("/tmp/logs/stderr.log")),
        };

        let in_process = spawn_outcome(SpawnOutcome::Spawned(payload.clone()))
            .expect("in-process spawn mapper succeeds");
        let socket = spawned_process(payload).expect("socket spawn mapper succeeds");

        assert_eq!(in_process, socket);
        assert_eq!(in_process.lifecycle, lifecycle);
        assert_eq!(in_process.runtime_pid, 42);
    }

    #[test]
    fn runtime_doctor_report_marks_clean_doctor_ok() {
        let report = runtime_doctor_report(runtime_doctor(Vec::new()), Some("sock".to_string()));
        assert_eq!(report.status, "ok");
        assert_eq!(report.socket_path.as_deref(), Some("sock"));
        assert!(report.doctor.is_some());
        assert!(report.code.is_none());
        assert!(report.message.is_none());
    }

    #[test]
    fn runtime_doctor_report_marks_pending_migrations_degraded() {
        let report = runtime_doctor_report(runtime_doctor(vec!["pending".to_string()]), None);
        assert_eq!(report.status, "degraded");
    }

    #[test]
    fn runtime_doctor_error_preserves_error_fields() {
        let report = runtime_doctor_error(
            Some("daemon_unavailable".to_string()),
            "missing socket".to_string(),
            Some("sock".to_string()),
        );
        assert_eq!(report.status, "error");
        assert!(report.doctor.is_none());
        assert_eq!(report.socket_path.as_deref(), Some("sock"));
        assert_eq!(report.code.as_deref(), Some("daemon_unavailable"));
        assert_eq!(report.message.as_deref(), Some("missing socket"));
    }

    fn runtime_doctor(pending_descriptions: Vec<String>) -> RuntimeDoctorResponse {
        RuntimeDoctorResponse {
            version: version_info(),
            socket_path: "sock".to_string(),
            uptime_secs: 0,
            sqlite: MigrationState {
                applied: 1,
                total: 1 + pending_descriptions.len(),
                applied_descriptions: vec!["init".to_string()],
                pending_descriptions,
            },
            lifecycles: LifecycleCounts::default(),
            watchers: WatcherCounts {
                process_exit_watchers: 0,
                shim_sockets: 0,
                event_waiters: 0,
            },
            launchers: Vec::new(),
            tmux: TmuxStatus {
                available: true,
                version: Some("tmux 3.5".to_string()),
                error: None,
            },
            docker: Box::new(DockerStatus::legacy_missing()),
            log_availability: Vec::new(),
            last_probe_sweep: None,
            recent_lost: Vec::new(),
        }
    }
}
