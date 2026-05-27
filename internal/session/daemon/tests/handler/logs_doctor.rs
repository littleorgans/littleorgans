use chrono::Utc;

use crate::common::{
    LOCAL_UID, OrPanic as _, TestDaemon, local_context, mock_rtmd_doctor, runtime_doctor_response,
    spawn_test_session,
};
use lilo_session_core::{
    DoctorRequest, LogsRequest, LostEvidence, RpcRequest, RpcResponse, Selector, WaitCondition,
    WaitRequest,
};

#[tokio::test]
pub(crate) async fn spawn_persists_driver_stdout_path_for_logs() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let transcript = daemon.dir.path().join("runtime.stdout.log");
    std::fs::write(&transcript, "daemon spawned\n").or_panic("transcript writes");
    daemon.driver.set_spawn_stdout_path(transcript.clone());

    let session = spawn_test_session(&daemon, &context, "engineer").await;

    assert_eq!(
        session.transcript_path.as_deref(),
        Some(transcript.as_path())
    );
    let logs = daemon
        .state
        .handle(
            context,
            RpcRequest::Logs {
                request: LogsRequest {
                    selector: Selector::Id { id: session.id },
                    max_bytes: None,
                },
            },
        )
        .await;
    let RpcResponse::Logs { response } = logs.response else {
        panic!("expected logs response");
    };
    assert_eq!(response.content, "daemon spawned\n");
}

#[tokio::test]
pub(crate) async fn logs_wait_and_doctor_polish_paths_work() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let session = spawn_test_session(&daemon, &context, "engineer").await;
    let transcript = daemon.dir.path().join("transcript.jsonl");
    std::fs::write(&transcript, "first\nsecond\n").or_panic("transcript writes");
    daemon
        .state
        .store
        .record_transcript_path(&session.id, &transcript, Utc::now())
        .await
        .or_panic("transcript path records")
        .or_panic("session exists");

    let logs = daemon
        .state
        .handle(
            context.clone(),
            RpcRequest::Logs {
                request: LogsRequest {
                    selector: Selector::Id { id: session.id },
                    max_bytes: None,
                },
            },
        )
        .await;
    let RpcResponse::Logs { response } = logs.response else {
        panic!("expected logs response");
    };
    assert_eq!(response.content, "first\nsecond\n");

    let waited = daemon
        .state
        .handle(
            context.clone(),
            RpcRequest::Wait {
                request: WaitRequest {
                    selector: Selector::Id { id: session.id },
                    condition: WaitCondition::Running,
                    timeout_secs: 0,
                },
            },
        )
        .await;
    let RpcResponse::Wait { response } = waited.response else {
        panic!("expected wait response");
    };
    assert!(response.matched);

    daemon
        .state
        .store
        .mark_session_lost(&session.id, LostEvidence::PidNotAlive, chrono::Utc::now())
        .await
        .or_panic("session marks lost");
    let doctor = daemon
        .state
        .handle(
            context,
            RpcRequest::Doctor {
                request: DoctorRequest::default(),
            },
        )
        .await;
    let RpcResponse::Doctor { response } = doctor.response else {
        panic!("expected doctor response");
    };
    assert_eq!(response.status, "degraded");
    assert_eq!(
        response.findings[0].session_id,
        Some(session.id.to_string())
    );
    assert!(response.findings[0].message.contains("PidNotAlive"));
}

#[tokio::test]
pub(crate) async fn doctor_includes_runtime_matters_payload() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let (socket_path, server) = mock_rtmd_doctor(runtime_doctor_response());
    let state = daemon.state.with_rtmd_socket_path(socket_path);

    let doctor = state
        .handle(
            context,
            RpcRequest::Doctor {
                request: DoctorRequest::default(),
            },
        )
        .await;
    let RpcResponse::Doctor { response } = doctor.response else {
        panic!("expected doctor response");
    };

    assert_eq!(response.status, "ok");
    assert!(response.runtime.starts_with("rtmd (lilo-rm-client 0.6.x"));
    assert_eq!(response.runtime_matters.status, "ok");
    assert_eq!(
        response
            .runtime_matters
            .doctor
            .or_panic("runtime doctor payload")
            .watchers
            .process_exit_watchers,
        1
    );
    server.await.or_panic("rtmd doctor server");
}

#[tokio::test]
pub(crate) async fn doctor_reports_runtime_matters_unavailable() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let socket_path = daemon.dir.path().join("missing-rtmd.sock");
    let state = daemon.state.with_rtmd_socket_path(socket_path);

    let doctor = state
        .handle(
            context,
            RpcRequest::Doctor {
                request: DoctorRequest::default(),
            },
        )
        .await;
    let RpcResponse::Doctor { response } = doctor.response else {
        panic!("expected doctor response");
    };

    assert_eq!(response.status, "degraded");
    assert_eq!(response.runtime_matters.status, "error");
    assert_eq!(
        response.runtime_matters.code.as_deref(),
        Some("runtime_unavailable")
    );
    assert!(
        response.findings[0]
            .message
            .contains("runtime-matters doctor failed")
    );
}
