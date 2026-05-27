mod common;
use common::{LOCAL_UID, TestDaemon, local_context};
use lilo_rm_core::{CaptureError, CaptureResponse, PaneSnapshot};
use lilo_session_core::{
    CaptureRequest, IsolationPolicy, RpcResponse, RuntimeKind, SessionRpc, SpawnRequest,
};

#[tokio::test]
async fn spawn_validates_target_and_persists_tmux_pane() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    daemon.driver.set_spawn_tmux_pane("test:0.0");

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
                    target: "tmux:test:0.0".to_string(),
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
    assert_eq!(response.session.tmux_pane.as_deref(), Some("test:0.0"));
    assert_eq!(daemon.driver.launches()[0].target, "tmux:test:0.0");
}

#[tokio::test]
async fn spawn_rejects_invalid_target_before_launch() {
    let daemon = TestDaemon::new(LOCAL_UID).await;

    let response = spawn_with_target(&daemon, "tmux:not-a-pane").await;

    let RpcResponse::Error { message } = response.response else {
        panic!("expected target validation error");
    };
    assert!(message.contains("invalid runtime target"), "{message}");
    assert!(daemon.driver.launches().is_empty());
}

#[tokio::test]
async fn spawn_rejects_tmux_pane_dead_target_before_launch() {
    let daemon = TestDaemon::new(LOCAL_UID).await;

    let response = spawn_with_target(&daemon, "tmux:dead:0.0").await;

    let RpcResponse::Error { message } = response.response else {
        panic!("expected target validation error");
    };
    assert!(message.contains("tmux pane is unavailable"), "{message}");
    assert!(daemon.driver.launches().is_empty());
}

#[tokio::test]
async fn spawn_rejects_unsupported_target_before_launch() {
    let daemon = TestDaemon::new(LOCAL_UID).await;

    let response = spawn_with_target(&daemon, "ssh:host").await;

    let RpcResponse::Error { message } = response.response else {
        panic!("expected target validation error");
    };
    assert!(message.contains("unsupported runtime target"), "{message}");
    assert!(daemon.driver.launches().is_empty());
}

#[tokio::test]
async fn capture_delegates_to_driver_for_selected_session() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    daemon
        .driver
        .set_capture(CaptureResponse::Captured(PaneSnapshot {
            content: "pane text\n".to_string(),
            captured_at_ms: 10,
            scrollback_lines_requested: 20,
            scrollback_lines_included: 1,
            pane_history_lines: 1,
        }));
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
    let CaptureResponse::Captured(snapshot) = response.capture else {
        panic!("expected captured snapshot");
    };
    assert_eq!(snapshot.content, "pane text\n");
}

#[tokio::test]
async fn capture_surfaces_pane_unavailable_failure() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    daemon
        .driver
        .set_capture(CaptureResponse::Failed(CaptureError::PaneUnavailable));
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
        CaptureResponse::Failed(CaptureError::PaneUnavailable)
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
