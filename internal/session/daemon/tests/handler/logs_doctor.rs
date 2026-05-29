use chrono::Utc;

use crate::common::{LOCAL_UID, OrPanic as _, TestDaemon, local_context, spawn_test_session};
use lilo_session_core::{
    DoctorRequest, LogsRequest, LostEvidence, RpcResponse, Selector, SessionRpc, WaitCondition,
    WaitRequest,
};
use lilo_session_driver::RtmdDriver;
use std::sync::Arc;

#[tokio::test]
pub(crate) async fn spawn_persists_runtime_stdout_path_for_logs() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();

    let session = spawn_test_session(&daemon, &context, "engineer").await;
    let transcript = session
        .transcript_path
        .as_ref()
        .or_panic("runtime stdout path records");

    assert_eq!(
        transcript.file_name().and_then(|name| name.to_str()),
        Some("stdout.log")
    );
    assert!(transcript.starts_with(daemon.dir.path().join("lilo/logs/runtimes")));

    let mut log_body = String::new();
    for _ in 0..100 {
        let logs = daemon
            .state
            .handle(
                context.clone(),
                SessionRpc::Logs {
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
        log_body = response.content;
        if log_body.contains("lilo fake runtime ready") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        log_body.contains("lilo fake runtime ready"),
        "{}: {log_body}",
        transcript.display()
    );
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
            SessionRpc::Logs {
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
            SessionRpc::Wait {
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
            SessionRpc::Doctor {
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
    let state = daemon.in_process_state_without_rtmd_socket_path().await;

    let doctor = state
        .handle(
            context,
            SessionRpc::Doctor {
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
    assert_eq!(response.runtime_matters.socket_path, None);
    assert_eq!(response.runtime_matters.code, None);
    assert_eq!(
        response
            .runtime_matters
            .doctor
            .or_panic("runtime doctor payload")
            .watchers
            .process_exit_watchers,
        0
    );
}

#[tokio::test]
pub(crate) async fn doctor_reports_runtime_matters_unavailable() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let socket_path = daemon.dir.path().join("missing-rtmd.sock");
    let state = daemon
        .state_with_runtime_port(Arc::new(RtmdDriver::new(socket_path)))
        .await;

    let doctor = state
        .handle(
            context,
            SessionRpc::Doctor {
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
