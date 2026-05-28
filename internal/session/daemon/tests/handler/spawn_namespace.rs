use chrono::Utc;

use crate::common::{
    LOCAL_UID, OrPanic as _, TestDaemon, handle_spawn, headless_spawn_request, local_context,
    namespace_spawn_request,
};
use lilo_session_core::{Namespace, RpcResponse, Session, SpawnRequest};

#[tokio::test]
pub(crate) async fn spawn_accepts_new_dir_and_namespace_without_legacy_workspace() {
    assert_spawn_uses_requested_dir(String::new()).await;
}

#[tokio::test]
pub(crate) async fn spawn_prefers_new_dir_when_legacy_workspace_is_also_present() {
    let legacy_workspace = tempfile::tempdir().or_panic("legacy workspace creates");
    assert_spawn_uses_requested_dir(legacy_workspace.path().display().to_string()).await;
}

async fn assert_spawn_uses_requested_dir(workspace: String) {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let namespace = create_namespace(&daemon, "alpha").await;
    let dir = daemon.dir.path().display().to_string();
    let mut request = namespace_spawn_request("pm", dir.clone(), namespace.clone());
    request.workspace = workspace;

    let session = spawn_session(&daemon, request).await;

    assert_eq!(session.workspace, dir);
    assert_eq!(session.namespace, namespace);
    assert_eq!(session.dir, daemon.dir.path());
}

#[tokio::test]
pub(crate) async fn spawn_rejects_unknown_namespace_before_launch() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let namespace = Namespace::new("missing").or_panic("namespace validates");

    let spawned = handle_spawn(
        &daemon,
        local_context(),
        namespace_spawn_request("pm", daemon.dir.path().display().to_string(), namespace),
    )
    .await;

    let RpcResponse::Error { message } = spawned.response else {
        panic!("expected error response");
    };
    assert!(message.contains("namespace not found: missing"));
}

#[tokio::test]
pub(crate) async fn spawn_persists_dir_as_received_without_daemon_canonicalisation() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let child = daemon.dir.path().join("child");
    std::fs::create_dir(&child).or_panic("child dir creates");
    let raw_dir = child.join("..").display().to_string();
    let mut request = headless_spawn_request("pm", String::new());
    request.dir = Some(raw_dir.clone());

    let session = spawn_session(&daemon, request).await;
    let session_namespace = daemon
        .state
        .store
        .get_session_namespace(&session.id)
        .await
        .or_panic("session namespace loads")
        .or_panic("session namespace exists");
    assert_eq!(session_namespace.dir.display().to_string(), raw_dir);
}

async fn spawn_session(daemon: &TestDaemon, request: SpawnRequest) -> Session {
    let spawned = handle_spawn(daemon, local_context(), request).await;
    let RpcResponse::Spawned { response } = spawned.response else {
        panic!("expected spawn response");
    };
    response.session
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
