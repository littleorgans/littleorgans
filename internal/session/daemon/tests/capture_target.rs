mod common;
use common::{LOCAL_UID, TestDaemon, local_context};
use lilo_rm_core::{CaptureError, CaptureResponse};
use lilo_session_core::{
    CaptureRequest, IsolationPolicy, RpcResponse, RuntimeKind, SessionRpc, SpawnRequest,
};

#[tokio::test]
async fn spawn_headless_uses_runtime_service_without_driver_fallback() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();

    let response = daemon
        .state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "engineer".to_string(),
                    workspace: daemon.dir.path().display().to_string(),
                    dir: None,
                    namespace: None,
                    target: "headless".to_string(),
                    agent_config: None,
                    isolation: IsolationPolicy::default(),
                    image: None,
                    env: Vec::new(),
                    mounts: Vec::new(),
                    shell_resume: None,
                    labels: Vec::new(),
                    force: false,
                }),
            },
        )
        .await;

    let RpcResponse::Spawned { response } = response.response else {
        panic!("expected spawn response");
    };
    assert_eq!(response.session.tmux_pane, None);
    assert!(response.session.runtime_pid > 0);
}

#[tokio::test]
async fn spawn_rejects_invalid_target_before_launch() {
    let daemon = TestDaemon::new(LOCAL_UID).await;

    let response = spawn_with_target(&daemon, "tmux:not-a-pane").await;

    let RpcResponse::Error { message } = response.response else {
        panic!("expected target validation error");
    };
    assert!(message.contains("invalid runtime target"), "{message}");
}

#[tokio::test]
async fn spawn_rejects_tmux_pane_dead_target_before_launch() {
    let daemon = TestDaemon::new(LOCAL_UID).await;

    let response = spawn_with_target(&daemon, "tmux:dead:0.0").await;

    let RpcResponse::Error { message } = response.response else {
        panic!("expected target validation error");
    };
    assert!(
        message.contains("tmux address dead:0.0 is not alive"),
        "{message}"
    );
}

#[tokio::test]
async fn spawn_rejects_unsupported_target_before_launch() {
    let daemon = TestDaemon::new(LOCAL_UID).await;

    let response = spawn_with_target(&daemon, "ssh:host").await;

    let RpcResponse::Error { message } = response.response else {
        panic!("expected target validation error");
    };
    assert!(
        message.contains("invalid runtime target: ssh:host"),
        "{message}"
    );
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
    daemon
        .state
        .handle(
            local_context(),
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "engineer".to_string(),
                    workspace: daemon.dir.path().display().to_string(),
                    dir: None,
                    namespace: None,
                    target: target.to_string(),
                    agent_config: None,
                    isolation: IsolationPolicy::default(),
                    image: None,
                    env: Vec::new(),
                    mounts: Vec::new(),
                    shell_resume: None,
                    labels: Vec::new(),
                    force: false,
                }),
            },
        )
        .await
}
