mod common;
use common::OrPanic as _;

use common::{LOCAL_UID, TestDaemon, local_context};
use lilo_session_core::{
    IsolationPolicy, Namespace, NamespaceCreateRequest, NamespaceDeleteRequest,
    NamespaceGetRequest, NamespaceListRequest, RpcResponse, RuntimeKind, Selector, SessionRpc,
    SpawnRequest,
};
use lilo_session_daemon::identity_client::RequestContext;

#[tokio::test]
async fn namespace_create_get_and_list_are_idempotent() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();

    let created = daemon
        .state
        .handle(
            context.clone(),
            SessionRpc::NamespaceCreate {
                request: NamespaceCreateRequest {
                    slug: "alpha".to_string(),
                },
            },
        )
        .await;
    let RpcResponse::NamespaceCreated { response } = created.response else {
        panic!("expected namespace create response");
    };
    assert!(response.created);
    assert_eq!(response.namespace.namespace.as_str(), "alpha");

    let recreated = daemon
        .state
        .handle(
            context.clone(),
            SessionRpc::NamespaceCreate {
                request: NamespaceCreateRequest {
                    slug: "alpha".to_string(),
                },
            },
        )
        .await;
    let RpcResponse::NamespaceCreated { response } = recreated.response else {
        panic!("expected namespace create response");
    };
    assert!(!response.created);
    assert_eq!(response.namespace.namespace.as_str(), "alpha");

    let got = daemon
        .state
        .handle(
            context.clone(),
            SessionRpc::NamespaceGet {
                request: NamespaceGetRequest {
                    slug: "alpha".to_string(),
                },
            },
        )
        .await;
    let RpcResponse::NamespaceGot { response } = got.response else {
        panic!("expected namespace get response");
    };
    assert_eq!(
        response
            .namespace
            .or_panic("namespace exists")
            .namespace
            .as_str(),
        "alpha"
    );

    let listed = daemon
        .state
        .handle(
            context,
            SessionRpc::NamespaceList {
                request: NamespaceListRequest::default(),
            },
        )
        .await;
    let RpcResponse::NamespacesListed { response } = listed.response else {
        panic!("expected namespace list response");
    };
    assert_eq!(
        response
            .namespaces
            .iter()
            .map(|record| record.namespace.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "default"]
    );
}

#[tokio::test]
async fn namespace_create_rejects_reserved_and_bad_slugs() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();

    for slug in ["default", "Alpha"] {
        let created = daemon
            .state
            .handle(
                context.clone(),
                SessionRpc::NamespaceCreate {
                    request: NamespaceCreateRequest {
                        slug: slug.to_string(),
                    },
                },
            )
            .await;
        let RpcResponse::Error { message } = created.response else {
            panic!("expected namespace create error");
        };
        assert!(message.contains("namespace"));
    }
}

#[tokio::test]
async fn spawn_uses_strict_create_before_spawn_policy() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();

    let missing = daemon
        .state
        .handle(
            context.clone(),
            SessionRpc::Spawn {
                request: Box::new(spawn_request(
                    daemon.dir.path().display().to_string(),
                    Namespace::new("alpha").or_panic("namespace validates"),
                )),
            },
        )
        .await;
    let RpcResponse::Error { message } = missing.response else {
        panic!("expected spawn error");
    };
    assert!(message.contains("namespace not found: alpha"));

    daemon
        .state
        .handle(
            context.clone(),
            SessionRpc::NamespaceCreate {
                request: NamespaceCreateRequest {
                    slug: "alpha".to_string(),
                },
            },
        )
        .await;

    let spawned = daemon
        .state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(spawn_request(
                    daemon.dir.path().display().to_string(),
                    Namespace::new("alpha").or_panic("namespace validates"),
                )),
            },
        )
        .await;
    let RpcResponse::Spawned { response } = spawned.response else {
        panic!("expected spawn response");
    };
    assert_eq!(response.session.namespace.as_str(), "alpha");
}

#[tokio::test]
async fn namespace_delete_terminates_and_removes_namespace_sessions() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    create_namespace(&daemon, &context, "alpha").await;

    let spawned = daemon
        .state
        .handle(
            context.clone(),
            SessionRpc::Spawn {
                request: Box::new(spawn_request(
                    daemon.dir.path().display().to_string(),
                    Namespace::new("alpha").or_panic("namespace validates"),
                )),
            },
        )
        .await;
    let RpcResponse::Spawned { response } = spawned.response else {
        panic!("expected spawn response");
    };
    let session_id = response.session.id;

    let deleted = daemon
        .state
        .handle(
            context.clone(),
            SessionRpc::NamespaceDelete {
                request: NamespaceDeleteRequest {
                    namespace: Namespace::new("alpha").or_panic("namespace validates"),
                },
            },
        )
        .await;
    let RpcResponse::NamespaceDeleted { response } = deleted.response else {
        panic!("expected namespace delete response");
    };
    assert_eq!(response.namespace.as_str(), "alpha");
    assert_eq!(response.sessions.len(), 1);
    assert!(response.sessions[0].terminated_at.is_some());

    let listed = daemon
        .state
        .handle(
            context,
            SessionRpc::List {
                request: lilo_session_core::ListRequest {
                    selector: Some(Selector::Namespace {
                        namespace: Namespace::new("alpha").or_panic("namespace validates"),
                    }),
                },
            },
        )
        .await;
    let RpcResponse::Listed { response } = listed.response else {
        panic!("expected list response");
    };
    assert!(
        response.sessions.is_empty(),
        "session {session_id} remained"
    );
}

#[tokio::test]
async fn namespace_delete_rejects_default_and_unknown_namespaces() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();

    for namespace in ["default", "missing"] {
        let deleted = daemon
            .state
            .handle(
                context.clone(),
                SessionRpc::NamespaceDelete {
                    request: NamespaceDeleteRequest {
                        namespace: Namespace::new(namespace).or_panic("namespace validates"),
                    },
                },
            )
            .await;
        let RpcResponse::Error { message } = deleted.response else {
            panic!("expected namespace delete error");
        };
        assert!(message.contains("namespace"));
    }
}

async fn create_namespace(daemon: &TestDaemon, context: &RequestContext, slug: &str) {
    let created = daemon
        .state
        .handle(
            context.clone(),
            SessionRpc::NamespaceCreate {
                request: NamespaceCreateRequest {
                    slug: slug.to_string(),
                },
            },
        )
        .await;
    let RpcResponse::NamespaceCreated { .. } = created.response else {
        panic!("expected namespace create response");
    };
}

fn spawn_request(dir: String, namespace: Namespace) -> SpawnRequest {
    SpawnRequest {
        runtime: RuntimeKind::Claude,
        role: "pm".to_string(),
        workspace: String::new(),
        dir: Some(dir),
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
    }
}
