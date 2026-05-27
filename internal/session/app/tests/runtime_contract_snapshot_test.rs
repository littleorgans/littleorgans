mod common;

use chrono::{TimeZone, Utc};
use common::OrPanic as _;
use lilo_rm_core::{
    CaptureError, CapturePayload, CaptureResponse, CursorExpiredPayload, DoctorPayload, EventBatch,
    EventsPayload, IsolationPolicy, KillOutcome, KilledPayload, Lifecycle, LifecycleCounts,
    LifecycleLogAvailability, LogAvailability, LostEvidence, MigrationState, NudgeFailureReason,
    NudgeOutcome, NudgePayload, NudgeResponse, PaneSnapshot, RecentLostEvent, RuntimeEvent,
    RuntimeKind, RuntimeResponse, SpawnedPayload, StatusPayload, TerminationEvidence, TmuxStatus,
    ValidateTargetOutcome, ValidateTargetPayload, ValidateTargetResponse, VersionInfo,
    WatcherCounts,
};
use uuid::Uuid;

#[test]
fn rtmd_payload_json_shapes_are_snapshotted() {
    let session_id = session_id();
    let lifecycle = lifecycle(session_id);

    insta::assert_json_snapshot!(serde_json::json!({
        "spawn": RuntimeResponse::Spawned(SpawnedPayload {
            lifecycle: lifecycle.clone(),
            event: running_event(session_id),
            log_dir: Some("/tmp/rtm/logs/018f6e28-0000-7000-8000-000000000001".into()),
            stdout_path: Some("/tmp/rtm/logs/018f6e28-0000-7000-8000-000000000001/stdout.log".into()),
            stderr_path: Some("/tmp/rtm/logs/018f6e28-0000-7000-8000-000000000001/stderr.log".into()),
        }),
        "status": RuntimeResponse::Status(StatusPayload {
            lifecycles: vec![lifecycle.clone()],
        }),
        "kill": RuntimeResponse::Killed(KilledPayload {
            outcome: KillOutcome::AlreadyExited,
        }),
        "nudge": RuntimeResponse::Nudge(NudgePayload {
            response: NudgeResponse {
                delivered: false,
                outcome: NudgeOutcome::Unsupported(NudgeFailureReason::HeadlessLifecycle),
            },
        }),
        "validate_target": RuntimeResponse::ValidateTarget(ValidateTargetPayload {
            response: ValidateTargetResponse {
                valid: false,
                outcome: ValidateTargetOutcome::TmuxPaneDead {
                    address: "agents:0.1".parse().or_panic("tmux address"),
                },
            },
        }),
        "capture": RuntimeResponse::Capture(CapturePayload {
            response: CaptureResponse::Captured(PaneSnapshot {
                content: "ready\n".to_string(),
                captured_at_ms: 1_700_000_001_000,
                scrollback_lines_requested: 500,
                scrollback_lines_included: 1,
                pane_history_lines: 42,
            }),
        }),
        "capture_failed": RuntimeResponse::Capture(CapturePayload {
            response: CaptureResponse::Failed(CaptureError::PaneUnavailable),
        }),
        "events": EventBatch::Events {
            events: vec![
                running_event(session_id),
                RuntimeEvent::Terminated {
                    session_id,
                    exit_code: Some(0),
                    signal: None,
                    evidence: TerminationEvidence::ProcessExit,
                },
            ],
            cursor: 8,
        },
        "cursor_expired": EventBatch::CursorExpired { oldest: 5 },
        "events_response": RuntimeResponse::Events(EventsPayload {
            events: vec![RuntimeEvent::Lost {
                session_id,
                evidence: LostEvidence::PidNotAlive,
            }],
            cursor: 9,
        }),
        "cursor_expired_response": RuntimeResponse::CursorExpired(CursorExpiredPayload {
            oldest: 5,
        }),
        "doctor": RuntimeResponse::Doctor(DoctorPayload {
            doctor: doctor_payload(session_id),
        }),
    }));
}

fn lifecycle(session_id: Uuid) -> Lifecycle {
    Lifecycle {
        session_id,
        runtime: RuntimeKind::Claude,
        isolation: IsolationPolicy::default(),
        state: lilo_rm_core::LifecycleState::Running,
        shim_pid: Some(4241),
        runtime_pid: Some(4242),
        start_time: Some(timestamp()),
        tmux_pane: Some("agents:0.1".parse().or_panic("tmux address")),
        log_availability: Some(LogAvailability::TmuxPaneSnapshot),
    }
}

fn running_event(session_id: Uuid) -> RuntimeEvent {
    RuntimeEvent::Running {
        session_id,
        runtime_pid: 4242,
        start_time: timestamp(),
    }
}

fn doctor_payload(session_id: Uuid) -> lilo_rm_core::DoctorResponse {
    lilo_rm_core::DoctorResponse {
        version: VersionInfo::new("0.6.0", "0123456"),
        socket_path: "/tmp/rtm/sock".to_string(),
        uptime_secs: 12,
        sqlite: MigrationState {
            applied: 2,
            total: 2,
            applied_descriptions: vec!["lifecycle".to_string(), "events".to_string()],
            pending_descriptions: Vec::new(),
        },
        lifecycles: LifecycleCounts {
            forking: 0,
            running: 1,
            exited: 2,
            lost: 0,
        },
        watchers: WatcherCounts {
            process_exit_watchers: 1,
            shim_sockets: 1,
            event_waiters: 0,
        },
        launchers: Vec::new(),
        tmux: TmuxStatus {
            available: true,
            version: Some("tmux 3.5a".to_string()),
            error: None,
        },
        docker: Box::new(lilo_rm_core::DockerStatus::legacy_missing()),
        log_availability: vec![LifecycleLogAvailability {
            session_id,
            log_availability: LogAvailability::Headless {
                stdout_path: "/tmp/rtm/stdout.log".into(),
                stderr_path: "/tmp/rtm/stderr.log".into(),
            },
        }],
        last_probe_sweep: Some(timestamp()),
        recent_lost: vec![RecentLostEvent {
            session_id,
            evidence: LostEvidence::PidReuseDetected,
            occurred_at: timestamp(),
        }],
    }
}

fn session_id() -> Uuid {
    Uuid::parse_str("018f6e28-0000-7000-8000-000000000001").or_panic("uuid")
}

fn timestamp() -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0)
        .single()
        .or_panic("timestamp is valid")
}
