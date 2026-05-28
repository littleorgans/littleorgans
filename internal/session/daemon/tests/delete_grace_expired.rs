mod common;

use common::{LOCAL_UID, TestDaemon, local_context, spawn_test_session};
use lilo_session_core::{DeleteRequest, RpcResponse, Selector, SessionRpc, SessionState};

#[tokio::test]
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
}
