mod common;

use common::{LOCAL_UID, TestDaemon, local_context, spawn_test_session};
use lilo_session_core::{DeleteRequest, RpcResponse, Selector, SessionRpc, SessionState};

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn delete_persists_runtime_termination() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let session = spawn_test_session(&daemon, &context, "engineer").await;

    let deleted = daemon
        .state
        .handle(
            context,
            SessionRpc::Delete {
                request: DeleteRequest {
                    selector: Selector::Id { id: session.id },
                    signal: "SIGTERM".to_string(),
                    grace_secs: 0,
                },
            },
        )
        .await;
    let RpcResponse::Deleted { response } = deleted.response else {
        panic!("expected delete response");
    };

    assert!(response.errors.is_empty());
    assert_eq!(response.sessions.len(), 1);
    assert_eq!(response.sessions[0].id, session.id);
    assert_eq!(response.sessions[0].state, SessionState::Terminated);
    assert!(response.sessions[0].terminated_at.is_some());
    daemon.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn delete_rejects_invalid_signal_before_termination() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let session = spawn_test_session(&daemon, &context, "engineer").await;

    let deleted = daemon
        .state
        .handle(
            context,
            SessionRpc::Delete {
                request: DeleteRequest {
                    selector: Selector::Id { id: session.id },
                    signal: "not-a-signal".to_string(),
                    grace_secs: 0,
                },
            },
        )
        .await;
    let RpcResponse::Error { message } = deleted.response else {
        panic!("expected invalid signal error");
    };
    assert!(message.contains("invalid runtime signal"), "{message}");
    let stored = daemon
        .state
        .store
        .get_session(&session.id)
        .await
        .expect("session lookup succeeds")
        .expect("session remains");
    assert_eq!(stored.state, SessionState::Running);
    daemon.cleanup().await;
}
