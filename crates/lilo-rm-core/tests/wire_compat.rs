mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use lilo_rm_core::{
    CaptureError, CapturePayload, CaptureResponse, CursorExpiredPayload, DockerStatus,
    DoctorPayload, ErrorCode, ErrorPayload, EventsPayload, KillByPidPayload, KillByPidResponse,
    KillOutcome, KilledPayload, LaunchEnv, LaunchSpec, LauncherStatus, Lifecycle, LifecycleCounts,
    LifecycleLogAvailability, LogAvailability, McpBridgePayload, McpBridgeResponse, MigrationState,
    NudgeFailureReason, NudgeOutcome, NudgePayload, NudgeResponse, RuntimeCapability, RuntimeEvent,
    RuntimeKind, RuntimeResponse, ShimLaunchPayload, SpawnedPayload, StatusPayload, TmuxStatus,
    ValidateTargetOutcome, ValidateTargetPayload, ValidateTargetResponse, VersionInfo,
    VersionPayload, WatcherCounts, WatchersPayload,
};
use support::{other_session_id, session_id, timestamp};

const FIXTURES: [&str; 17] = [
    "ack.json",
    "capture.json",
    "cursor_expired.json",
    "doctor.json",
    "error.json",
    "events.json",
    "killed.json",
    "kill_by_pid.json",
    "mcp_bridge.json",
    "nudge.json",
    "shim_launch.json",
    "spawned.json",
    "status.json",
    "stopping.json",
    "validate_target.json",
    "version.json",
    "watchers.json",
];

#[test]
fn runtime_response_v0_5_wire_fixtures_round_trip() {
    assert_fixture_set();

    for (fixture, expected) in expected_responses() {
        let path = fixture_dir().join(fixture);
        let json = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", path.display());
        });
        let actual: RuntimeResponse = serde_json::from_str(&json).unwrap_or_else(|error| {
            panic!("failed to parse {}: {error}", path.display());
        });

        assert_eq!(actual, expected, "{fixture}");
        assert_eq!(
            serde_json::to_value(&actual).expect("serialize response"),
            serde_json::from_str::<serde_json::Value>(&json).expect("parse fixture value"),
            "{fixture}"
        );
    }
}

fn assert_fixture_set() {
    let actual = fs::read_dir(fixture_dir())
        .expect("fixture dir")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| {
            path.extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        })
        .map(|path| {
            path.file_name()
                .expect("fixture file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect::<BTreeSet<_>>();
    let expected = FIXTURES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);
}

fn expected_responses() -> Vec<(&'static str, RuntimeResponse)> {
    let session_id = session_id();
    let mut responses = expected_control_responses(session_id);
    responses.extend(expected_runtime_responses(session_id));
    responses.extend(expected_metadata_responses());
    responses
}

fn expected_control_responses(_session_id: uuid::Uuid) -> Vec<(&'static str, RuntimeResponse)> {
    vec![
        ("ack.json", RuntimeResponse::Ack),
        (
            "capture.json",
            RuntimeResponse::Capture(CapturePayload {
                response: CaptureResponse::Failed(CaptureError::NotATmuxTarget),
            }),
        ),
        (
            "cursor_expired.json",
            RuntimeResponse::CursorExpired(CursorExpiredPayload { oldest: 7 }),
        ),
        (
            "doctor.json",
            RuntimeResponse::Doctor(DoctorPayload {
                doctor: v05_doctor_response(),
            }),
        ),
        (
            "error.json",
            RuntimeResponse::Error(ErrorPayload {
                code: ErrorCode::RuntimeUnavailable,
                message: "no launcher registered for runtime kind: missing-runtime".to_owned(),
            }),
        ),
        (
            "events.json",
            RuntimeResponse::Events(EventsPayload {
                events: vec![RuntimeEvent::Lost {
                    session_id: other_session_id(),
                    evidence: lilo_rm_core::LostEvidence::PidNotAlive,
                }],
                cursor: 8,
            }),
        ),
        (
            "killed.json",
            RuntimeResponse::Killed(KilledPayload {
                outcome: KillOutcome::AlreadyExited,
            }),
        ),
        (
            "kill_by_pid.json",
            RuntimeResponse::KillByPid(KillByPidPayload {
                response: KillByPidResponse {
                    pid: 77689,
                    signal: 15,
                    killed_after_grace: false,
                    outcome: KillOutcome::Signalled,
                },
            }),
        ),
        (
            "mcp_bridge.json",
            RuntimeResponse::McpBridge(McpBridgePayload {
                response: McpBridgeResponse {
                    line: Some("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}".to_owned()),
                },
            }),
        ),
        (
            "nudge.json",
            RuntimeResponse::Nudge(NudgePayload {
                response: NudgeResponse {
                    delivered: false,
                    outcome: NudgeOutcome::Unsupported(NudgeFailureReason::HeadlessLifecycle),
                },
            }),
        ),
    ]
}

fn expected_runtime_responses(session_id: uuid::Uuid) -> Vec<(&'static str, RuntimeResponse)> {
    vec![
        (
            "shim_launch.json",
            RuntimeResponse::ShimLaunch(ShimLaunchPayload {
                launch: v05_launch_spec(),
            }),
        ),
        (
            "spawned.json",
            RuntimeResponse::Spawned(SpawnedPayload {
                lifecycle: v05_headless_lifecycle(session_id),
                event: RuntimeEvent::Running {
                    session_id,
                    runtime_pid: 1,
                    start_time: timestamp(),
                },
                log_dir: Some(v05_session_log_dir()),
                stdout_path: Some(v05_stdout_path()),
                stderr_path: Some(v05_stderr_path()),
            }),
        ),
        (
            "status.json",
            RuntimeResponse::Status(StatusPayload {
                lifecycles: vec![v05_headless_lifecycle(session_id)],
            }),
        ),
        ("stopping.json", RuntimeResponse::Stopping),
        (
            "validate_target.json",
            RuntimeResponse::ValidateTarget(ValidateTargetPayload {
                response: ValidateTargetResponse {
                    valid: false,
                    outcome: ValidateTargetOutcome::InvalidTarget {
                        message: "invalid spawn target tmux:not-a-pane; expected headless or tmux:<session>:<window>.<pane>"
                            .to_owned(),
                    },
                },
            }),
        ),
    ]
}

fn expected_metadata_responses() -> Vec<(&'static str, RuntimeResponse)> {
    vec![
        (
            "version.json",
            RuntimeResponse::Version(VersionPayload {
                version: v05_version_info(),
            }),
        ),
        (
            "watchers.json",
            RuntimeResponse::Watchers(WatchersPayload {
                watchers: v05_watcher_counts(),
            }),
        ),
    ]
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0_5")
}

fn v05_headless_lifecycle(session_id: uuid::Uuid) -> Lifecycle {
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    assert!(lifecycle.mark_running(lilo_rm_core::ShimReady {
        session_id,
        shim_pid: 1,
        runtime_pid: 1,
        start_time: timestamp(),
        tmux_pane: None,
    }));
    lifecycle.log_availability = Some(LogAvailability::Headless {
        stdout_path: v05_stdout_path(),
        stderr_path: v05_stderr_path(),
    });
    lifecycle
}

fn v05_session_log_dir() -> PathBuf {
    PathBuf::from(
        "/tmp/runtime-matters-v0.5-fixture-home/logs/018f6e28-0000-7000-8000-000000000001",
    )
}

fn v05_stdout_path() -> PathBuf {
    v05_session_log_dir().join("stdout.log")
}

fn v05_stderr_path() -> PathBuf {
    v05_session_log_dir().join("stderr.log")
}

fn v05_launch_spec() -> LaunchSpec {
    LaunchSpec {
        argv: vec!["/Users/alphab/.local/bin/claude".to_owned()],
        env: vec![
            LaunchEnv::new("RTM", "1"),
            LaunchEnv::new("HELIOY_SESSION_ID", session_id().to_string()),
            LaunchEnv::new("HELIOY_RUNTIME", "claude"),
            LaunchEnv::new("RTM_SESSION_ID", session_id().to_string()),
            LaunchEnv::new("RTM_RUNTIME_KIND", "claude"),
        ],
        cwd: "/tmp/rtm".into(),
        shell_resume: None,
    }
}

fn v05_doctor_response() -> lilo_rm_core::DoctorResponse {
    lilo_rm_core::DoctorResponse {
        version: v05_version_info(),
        socket_path: "/tmp/runtime-matters-v0.5-fixture-home/rtmd.sock".to_owned(),
        uptime_secs: 0,
        sqlite: MigrationState {
            applied: 2,
            total: 2,
            applied_descriptions: vec!["lifecycle".to_owned(), "probe state".to_owned()],
            pending_descriptions: Vec::new(),
        },
        lifecycles: LifecycleCounts {
            forking: 0,
            running: 1,
            exited: 0,
            lost: 0,
        },
        watchers: v05_watcher_counts(),
        launchers: vec![
            LauncherStatus {
                runtime: "claude".to_owned(),
                command: Some("/Users/alphab/.local/bin/claude".to_owned()),
                error: None,
            },
            LauncherStatus {
                runtime: "codex".to_owned(),
                command: Some(
                    "/Users/alphab/.local/share/mise/installs/node/25/bin/codex".to_owned(),
                ),
                error: None,
            },
        ],
        tmux: TmuxStatus {
            available: true,
            version: Some("tmux 3.6a".to_owned()),
            error: None,
        },
        docker: Box::new(DockerStatus::legacy_missing()),
        log_availability: vec![LifecycleLogAvailability {
            session_id: session_id(),
            log_availability: LogAvailability::Headless {
                stdout_path: v05_stdout_path(),
                stderr_path: v05_stderr_path(),
            },
        }],
        last_probe_sweep: Some(v05_probe_sweep()),
        recent_lost: Vec::new(),
    }
}

fn v05_version_info() -> VersionInfo {
    VersionInfo {
        version: "0.2.0".to_owned(),
        git_sha: "782b3e5e19c5".to_owned(),
        protocol_version: "0.4".to_owned(),
        capabilities: vec![
            RuntimeCapability::StructuredProtocolErrors,
            RuntimeCapability::HeadlessStdioLogPaths,
            RuntimeCapability::StatusSessionSetFilter,
            RuntimeCapability::StatusUpdatedSinceFilter,
            RuntimeCapability::TypedNudgeOutcomes,
            RuntimeCapability::ValidateTargetPreflight,
            RuntimeCapability::EventsCursor,
            RuntimeCapability::EventsLongPoll,
            RuntimeCapability::TmuxPaneSnapshot,
        ],
    }
}

fn v05_watcher_counts() -> WatcherCounts {
    WatcherCounts {
        process_exit_watchers: 1,
        shim_sockets: 0,
        event_waiters: 0,
    }
}

fn v05_probe_sweep() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-05-19T11:49:32.054125Z")
        .expect("v0.5 probe sweep")
        .with_timezone(&Utc)
}
