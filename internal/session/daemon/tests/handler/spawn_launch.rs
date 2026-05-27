use crate::common::{LOCAL_UID, TestDaemon, local_context, spawn_test_session};
use lilo_session_core::{
    IsolationPolicy, Namespace, RpcResponse, RuntimeKind, SessionRpc, SpawnRequest,
};

#[tokio::test]
pub(crate) async fn spawn_launch_uses_runtime_service_without_driver_fallback() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();

    let spawned = daemon
        .state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "pm".to_string(),
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

    let RpcResponse::Spawned { response } = spawned.response else {
        panic!("expected spawn response");
    };
    assert_eq!(response.session.runtime, RuntimeKind::Claude);
    assert_eq!(response.session.role, "pm");
    assert_eq!(
        response.session.workspace,
        daemon.dir.path().display().to_string()
    );
    assert_eq!(response.session.dir, daemon.dir.path());
    assert!(response.session.runtime_pid > 0);
    assert!(daemon.driver.launches().is_empty());
}

#[tokio::test]
pub(crate) async fn spawn_launch_cwd_is_request_workspace() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let session = spawn_test_session(&daemon, &local_context(), "pm").await;

    assert_eq!(session.namespace, Namespace::default());
    assert_eq!(session.workspace, daemon.dir.path().display().to_string());
    assert_eq!(session.dir, daemon.dir.path());
    assert!(session.runtime_pid > 0);
    assert!(daemon.driver.launches().is_empty());
}
