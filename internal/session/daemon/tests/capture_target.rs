mod common;
use common::{
    LOCAL_UID, TestDaemon, handle_spawn, headless_spawn_request, local_context, spawn_request,
};
use lilo_rm_core::{CaptureError, CaptureResponse};
use lilo_session_core::{CaptureRequest, RpcResponse, SessionRpc};

#[tokio::test]
async fn spawn_headless_uses_runtime_service_without_driver_fallback() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();

    let request = headless_spawn_request("engineer", daemon.dir.path().display().to_string());
    let response = handle_spawn(&daemon, context, request).await;

    let RpcResponse::Spawned { response } = response.response else {
        panic!("expected spawn response");
    };
    assert_eq!(response.session.tmux_pane, None);
    assert!(response.session.runtime_pid > 0);
}

#[tokio::test]
async fn spawn_rejects_invalid_target_before_launch() {
    assert_spawn_rejects_target("tmux:not-a-pane", "invalid runtime target").await;
}

#[tokio::test]
async fn spawn_rejects_tmux_pane_dead_target_before_launch() {
    assert_spawn_rejects_target("tmux:dead:0.0", "tmux address dead:0.0 is not alive").await;
}

#[tokio::test]
async fn spawn_rejects_unsupported_target_before_launch() {
    assert_spawn_rejects_target("ssh:host", "invalid runtime target: ssh:host").await;
}

async fn assert_spawn_rejects_target(target: &str, expected: &str) {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let response = spawn_with_target(&daemon, target).await;

    let RpcResponse::Error { message } = response.response else {
        panic!("expected target validation error");
    };
    assert!(message.contains(expected), "{message}");
}

#[tokio::test]
async fn capture_reports_runtime_headless_failure() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let session = common::spawn_test_session(&daemon, &context, "engineer").await;

    let response = daemon
        .state
        .handle(
            context,
            SessionRpc::Capture {
                request: CaptureRequest {
                    session_id: session.id,
                    scrollback_lines: Some(20),
                },
            },
        )
        .await;

    let RpcResponse::Capture { response } = response.response else {
        panic!("expected capture response");
    };
    assert_eq!(
        response.capture,
        CaptureResponse::Failed(CaptureError::NotATmuxTarget)
    );
}

async fn spawn_with_target(
    daemon: &TestDaemon,
    target: &str,
) -> lilo_session_daemon::handler::HandlerResult {
    handle_spawn(
        daemon,
        local_context(),
        spawn_request("engineer", daemon.dir.path().display().to_string(), target),
    )
    .await
}
