use chrono::Utc;

use crate::common::{LOCAL_UID, OrPanic as _, TestDaemon, local_context};
use lilo_session_core::{
    IsolationPolicy, Namespace, RpcResponse, RuntimeKind, SessionRpc, SpawnRequest,
};

#[tokio::test]
pub(crate) async fn spawn_accepts_new_dir_and_namespace_without_legacy_workspace() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let namespace = create_namespace(&daemon, "alpha").await;
    let dir = daemon.dir.path().display().to_string();

    let spawned = daemon
        .state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "pm".to_string(),
                    workspace: String::new(),
                    dir: Some(dir.clone()),
                    namespace: Some(namespace.clone()),
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
    assert_eq!(response.session.workspace, dir);
    assert_eq!(response.session.namespace, namespace);
    assert_eq!(response.session.dir, daemon.dir.path());
}

#[tokio::test]
pub(crate) async fn spawn_prefers_new_dir_when_legacy_workspace_is_also_present() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let namespace = create_namespace(&daemon, "alpha").await;
    let legacy_workspace = tempfile::tempdir().or_panic("legacy workspace creates");
    let dir = daemon.dir.path().display().to_string();

    let spawned = daemon
        .state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "pm".to_string(),
                    workspace: legacy_workspace.path().display().to_string(),
                    dir: Some(dir.clone()),
                    namespace: Some(namespace.clone()),
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
    assert_eq!(response.session.workspace, dir);
    assert_eq!(response.session.namespace, namespace);
    assert_eq!(response.session.dir, daemon.dir.path());
    assert!(daemon.driver.launches().is_empty());
}

#[tokio::test]
pub(crate) async fn spawn_rejects_unknown_namespace_before_launch() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let namespace = Namespace::new("missing").or_panic("namespace validates");

    let spawned = daemon
        .state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "pm".to_string(),
                    workspace: String::new(),
                    dir: Some(daemon.dir.path().display().to_string()),
                    namespace: Some(namespace),
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

    let RpcResponse::Error { message } = spawned.response else {
        panic!("expected error response");
    };
    assert!(message.contains("namespace not found: missing"));
    assert!(daemon.driver.launches().is_empty());
}

#[tokio::test]
pub(crate) async fn spawn_persists_dir_as_received_without_daemon_canonicalisation() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let child = daemon.dir.path().join("child");
    std::fs::create_dir(&child).or_panic("child dir creates");
    let raw_dir = child.join("..").display().to_string();

    let spawned = daemon
        .state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "pm".to_string(),
                    workspace: String::new(),
                    dir: Some(raw_dir.clone()),
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
    let session_namespace = daemon
        .state
        .store
        .get_session_namespace(&response.session.id)
        .await
        .or_panic("session namespace loads")
        .or_panic("session namespace exists");
    assert_eq!(session_namespace.dir.display().to_string(), raw_dir);
}

pub(crate) async fn create_namespace(daemon: &TestDaemon, value: &str) -> Namespace {
    let namespace = Namespace::for_create(value).or_panic("namespace validates");
    daemon
        .state
        .store
        .create_namespace(&namespace, Utc::now())
        .await
        .or_panic("namespace creates");
    namespace
}
