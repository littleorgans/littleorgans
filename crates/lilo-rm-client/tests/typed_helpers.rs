#[path = "common/mock_socket.rs"]
mod mock_socket;

use lilo_rm_client::{ClientError, request};
use lilo_rm_core::{
    CaptureError, CapturePayload, CaptureRequest, CaptureResponse, CursorExpiredPayload,
    DockerIsolationStatus, DockerReadiness, DockerStatus, DoctorPayload, DoctorResponse, ErrorCode,
    EventBatch, EventsPayload, EventsRequest, HeadlessSpawnTarget, IsolationPolicy,
    KillByPidPayload, KillByPidRequest, KillByPidResponse, KillOutcome, KillRequest, KilledPayload,
    Lifecycle, LifecycleCounts, MigrationState, NudgeFailureReason, NudgeOutcome, NudgePayload,
    NudgeRequest, NudgeResponse, RuntimeEvent, RuntimeKind, RuntimeResponse, RuntimeRpc,
    RuntimeSignal, SpawnConflictKind, SpawnConflictPayload, SpawnRequest, SpawnTarget,
    SpawnedPayload, StatusFilter, StatusPayload, ValidateTargetPayload, ValidateTargetRequest,
    ValidateTargetResponse, VersionInfo, VersionPayload, WatcherCounts,
};
use mock_socket::{mock_runtime_response as mock_response, temp_socket_path};
use uuid::Uuid;

#[tokio::test]
async fn missing_socket_reports_daemon_unavailable() {
    let (_tempdir, socket_path) = temp_socket_path();

    let error = request(&socket_path, RuntimeRpc::Version)
        .await
        .expect_err("missing socket should fail");

    match error {
        ClientError::DaemonUnavailable {
            socket_path: actual,
            ..
        } => assert_eq!(actual, socket_path),
        other => panic!("unexpected client error: {other:?}"),
    }
}

#[tokio::test]
async fn daemon_error_response_preserves_code() {
    let (client, server) = mock_response(
        RuntimeRpc::Version,
        RuntimeResponse::error(ErrorCode::SessionNotFound, "missing session"),
    );

    let error = client
        .request(RuntimeRpc::Version)
        .await
        .expect_err("daemon error response should fail");

    assert_error_response(error);
    server.await.expect("server task");
}

#[tokio::test]
async fn spawn_conflict_maps_to_typed_client_error() {
    let payload = SpawnConflictPayload {
        kind: SpawnConflictKind::SessionId,
        lifecycle: Lifecycle::forking(session_id(), RuntimeKind::Claude),
    };
    let (client, server) = mock_response(
        RuntimeRpc::Spawn {
            request: spawn_request(),
        },
        RuntimeResponse::SpawnConflict(payload.clone()),
    );

    let error = client
        .spawn(spawn_request())
        .await
        .expect_err("spawn conflict should fail");

    match error {
        ClientError::SpawnConflict(actual) => assert_eq!(*actual, payload),
        other => panic!("unexpected client error: {other:?}"),
    }
    server.await.expect("server task");
}

macro_rules! typed_helper_tests {
    ($module:ident, $method:ident($($arg:expr),*), $rpc:expr, $ok:expr, $value:expr, $expected:literal) => {
        mod $module {
            use super::*;

            #[tokio::test]
            async fn happy_path_returns_typed_value() {
                let (client, server) = mock_response($rpc, $ok);
                let actual = client.$method($($arg),*)
                    .await
                    .expect("typed helper should succeed");
                assert_eq!(actual, $value);
                server.await.expect("server task");
            }

            #[tokio::test]
            async fn daemon_error_maps_to_client_error_response() {
                let (client, server) = mock_response(
                    $rpc,
                    RuntimeResponse::error(ErrorCode::SessionNotFound, "missing session"),
                );
                let error = client.$method($($arg),*)
                    .await
                    .expect_err("daemon error should fail");
                assert_error_response(error);
                server.await.expect("server task");
            }

            #[tokio::test]
            async fn unexpected_variant_reports_expected_and_got() {
                let (client, server) = mock_response($rpc, unexpected_for($expected));
                let error = client.$method($($arg),*)
                    .await
                    .expect_err("unexpected response should fail");
                assert_unexpected(error, $expected, unexpected_name_for($expected));
                server.await.expect("server task");
            }
        }
    };
}

fn assert_error_response(error: ClientError) {
    match error {
        ClientError::ErrorResponse { code, message } => {
            assert_eq!(code, ErrorCode::SessionNotFound);
            assert_eq!(message, "missing session");
        }
        other => panic!("unexpected client error: {other:?}"),
    }
}

fn assert_unexpected(error: ClientError, expected: &'static str, got: &'static str) {
    match error {
        ClientError::UnexpectedResponse {
            expected: actual_expected,
            got: actual_got,
        } => {
            assert_eq!(actual_expected, expected);
            assert_eq!(actual_got, got);
        }
        other => panic!("unexpected client error: {other:?}"),
    }
}

fn unexpected_for(expected: &str) -> RuntimeResponse {
    if expected == "Version" {
        RuntimeResponse::Ack
    } else {
        RuntimeResponse::Version(version_payload())
    }
}

fn unexpected_name_for(expected: &str) -> &'static str {
    if expected == "Version" {
        "Ack"
    } else {
        "Version"
    }
}

fn session_id() -> Uuid {
    Uuid::parse_str("018f6e28-0000-7000-8000-000000000001").expect("uuid")
}

fn spawn_request() -> SpawnRequest {
    SpawnRequest {
        session_id: session_id(),
        runtime: RuntimeKind::Claude,
        isolation: IsolationPolicy::default(),
        image: None,
        env: Vec::new(),
        mounts: Vec::new(),
        cwd: "/tmp/rtm".into(),
        target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
        force: false,
        shell_resume: None,
    }
}

fn spawn_payload() -> SpawnedPayload {
    SpawnedPayload {
        lifecycle: Lifecycle::forking(session_id(), RuntimeKind::Claude),
        event: RuntimeEvent::Lost {
            session_id: session_id(),
            evidence: lilo_rm_core::LostEvidence::PidNotAlive,
        },
        log_dir: None,
        stdout_path: None,
        stderr_path: None,
    }
}

fn kill_request() -> KillRequest {
    KillRequest {
        session_id: session_id(),
        signal: RuntimeSignal::Term,
        grace_secs: 1,
    }
}

fn kill_by_pid_request() -> KillByPidRequest {
    KillByPidRequest {
        pid: 4242,
        signal: 15,
        grace_secs: 1,
    }
}

fn kill_by_pid_response() -> KillByPidResponse {
    KillByPidResponse {
        pid: 4242,
        signal: 15,
        killed_after_grace: false,
        outcome: KillOutcome::Signalled,
    }
}

fn nudge_request() -> NudgeRequest {
    NudgeRequest {
        session_id: session_id(),
        content: "wake up".to_owned(),
    }
}

fn nudge_payload() -> NudgePayload {
    NudgePayload {
        response: nudge_response(NudgeOutcome::Delivered),
    }
}

fn nudge_response(outcome: NudgeOutcome) -> NudgeResponse {
    NudgeResponse {
        delivered: matches!(outcome, NudgeOutcome::Delivered),
        outcome,
    }
}

fn unsupported_nudge_payload() -> NudgePayload {
    NudgePayload {
        response: NudgeResponse {
            delivered: false,
            outcome: NudgeOutcome::Unsupported(NudgeFailureReason::HeadlessLifecycle),
        },
    }
}

fn capture_request() -> CaptureRequest {
    CaptureRequest {
        session_id: session_id(),
        scrollback_lines: None,
    }
}

fn version_payload() -> VersionPayload {
    VersionPayload {
        version: VersionInfo::new("0.6.0", "test-sha"),
    }
}

fn doctor_payload() -> DoctorPayload {
    DoctorPayload {
        doctor: DoctorResponse {
            version: version_payload().version,
            socket_path: "/tmp/rtmd.sock".to_owned(),
            uptime_secs: 0,
            sqlite: MigrationState {
                applied: 0,
                total: 0,
                applied_descriptions: Vec::new(),
                pending_descriptions: Vec::new(),
            },
            lifecycles: LifecycleCounts::default(),
            watchers: WatcherCounts {
                process_exit_watchers: 0,
                shim_sockets: 0,
                event_waiters: 0,
            },
            launchers: Vec::new(),
            tmux: lilo_rm_core::TmuxStatus {
                available: false,
                version: None,
                error: None,
            },
            docker: Box::new(DockerStatus {
                cli: DockerReadiness::unavailable("docker unavailable"),
                daemon: DockerReadiness::unavailable("docker daemon unavailable"),
                manifest_validation: DockerReadiness::unavailable("docker manifest unavailable"),
                isolation: DockerIsolationStatus {
                    supported: true,
                    default_workspace: "/workspace".to_owned(),
                    experimental: true,
                },
            }),
            log_availability: Vec::new(),
            last_probe_sweep: None,
            recent_lost: Vec::new(),
        },
    }
}

typed_helper_tests!(
    spawn_helper,
    spawn(spawn_request()),
    RuntimeRpc::Spawn {
        request: spawn_request()
    },
    RuntimeResponse::Spawned(spawn_payload()),
    spawn_payload(),
    "Spawned"
);

typed_helper_tests!(
    kill_helper,
    kill(kill_request()),
    RuntimeRpc::Kill {
        request: kill_request()
    },
    RuntimeResponse::Killed(KilledPayload {
        outcome: KillOutcome::Signalled
    }),
    KillOutcome::Signalled,
    "Killed"
);

typed_helper_tests!(
    kill_by_pid_helper,
    kill_by_pid(kill_by_pid_request()),
    RuntimeRpc::KillByPid {
        request: kill_by_pid_request()
    },
    RuntimeResponse::KillByPid(KillByPidPayload {
        response: kill_by_pid_response()
    }),
    KillOutcome::Signalled,
    "KillByPid"
);

typed_helper_tests!(
    status_helper,
    status(StatusFilter::default()),
    RuntimeRpc::Status {
        request: StatusFilter::default().into()
    },
    RuntimeResponse::Status(StatusPayload {
        lifecycles: Vec::new()
    }),
    StatusPayload {
        lifecycles: Vec::new()
    },
    "Status"
);

typed_helper_tests!(
    nudge_helper,
    nudge(nudge_request()),
    RuntimeRpc::Nudge {
        request: nudge_request()
    },
    RuntimeResponse::Nudge(nudge_payload()),
    nudge_response(NudgeOutcome::Delivered),
    "Nudge"
);

async fn assert_nudge_helper_preserves_outcome(
    payload: NudgePayload,
    expected: NudgeResponse,
    message: &'static str,
) {
    let (client, server) = mock_response(
        RuntimeRpc::Nudge {
            request: nudge_request(),
        },
        RuntimeResponse::Nudge(payload),
    );

    let actual = client.nudge(nudge_request()).await.expect(message);

    assert_eq!(actual, expected);
    server.await.expect("server task");
}

#[tokio::test]
async fn nudge_helper_preserves_unsupported_headless_outcome() {
    assert_nudge_helper_preserves_outcome(
        unsupported_nudge_payload(),
        nudge_response(NudgeOutcome::Unsupported(
            NudgeFailureReason::HeadlessLifecycle,
        )),
        "unsupported nudge response is caller visible",
    )
    .await;
}

#[tokio::test]
async fn nudge_helper_preserves_tmux_pane_dead_outcome() {
    assert_nudge_helper_preserves_outcome(
        NudgePayload {
            response: nudge_response(NudgeOutcome::Failed(NudgeFailureReason::TmuxPaneDead)),
        },
        nudge_response(NudgeOutcome::Failed(NudgeFailureReason::TmuxPaneDead)),
        "failed nudge response is caller visible",
    )
    .await;
}

#[tokio::test]
async fn nudge_helper_preserves_terminal_session_outcome() {
    assert_nudge_helper_preserves_outcome(
        NudgePayload {
            response: nudge_response(NudgeOutcome::Failed(NudgeFailureReason::SessionEnded)),
        },
        nudge_response(NudgeOutcome::Failed(NudgeFailureReason::SessionEnded)),
        "terminal nudge response is caller visible",
    )
    .await;
}

typed_helper_tests!(
    capture_helper,
    capture(capture_request()),
    RuntimeRpc::Capture {
        request: capture_request()
    },
    RuntimeResponse::Capture(CapturePayload {
        response: CaptureResponse::Failed(CaptureError::PaneUnavailable)
    }),
    CaptureResponse::Failed(CaptureError::PaneUnavailable),
    "Capture"
);

typed_helper_tests!(
    validate_target_helper,
    validate_target("headless"),
    RuntimeRpc::ValidateTarget {
        request: ValidateTargetRequest {
            target: "headless".to_owned()
        }
    },
    RuntimeResponse::ValidateTarget(ValidateTargetPayload {
        response: ValidateTargetResponse::valid()
    }),
    ValidateTargetResponse::valid(),
    "ValidateTarget"
);

typed_helper_tests!(
    doctor_helper,
    doctor(),
    RuntimeRpc::Doctor,
    RuntimeResponse::Doctor(doctor_payload()),
    doctor_payload(),
    "Doctor"
);

typed_helper_tests!(
    version_helper,
    version(),
    RuntimeRpc::Version,
    RuntimeResponse::Version(version_payload()),
    version_payload(),
    "Version"
);

typed_helper_tests!(
    events_helper,
    events(EventsRequest::default()),
    RuntimeRpc::Events {
        request: EventsRequest::default()
    },
    RuntimeResponse::Events(EventsPayload {
        events: Vec::new(),
        cursor: 0
    }),
    EventBatch::Events {
        events: Vec::new(),
        cursor: 0
    },
    "Events or CursorExpired"
);

#[tokio::test]
async fn events_helper_returns_cursor_expired_batch() {
    let (client, server) = mock_response(
        RuntimeRpc::Events {
            request: EventsRequest {
                since: Some(5),
                wait_ms: Some(0),
            },
        },
        RuntimeResponse::CursorExpired(CursorExpiredPayload { oldest: 9 }),
    );

    let actual = client
        .events(EventsRequest {
            since: Some(5),
            wait_ms: Some(0),
        })
        .await
        .expect("cursor expired is a typed event batch");

    assert_eq!(actual, EventBatch::CursorExpired { oldest: 9 });
    server.await.expect("server task");
}
