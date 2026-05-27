use crate::common::{LOCAL_UID, TestDaemon, local_context, spawn_test_session};
use lilo_session_core::{
    DeleteRequest, Label, LabelMutation, LabelRequest, RpcResponse, Selector, SessionRpc,
    SessionState,
};

#[tokio::test]
pub(crate) async fn drives_session_through_delete_lifecycle() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let spawned = spawn_test_session(&daemon, &context, "general").await;

    let deleted = daemon
        .state
        .handle(
            context,
            SessionRpc::Delete {
                request: DeleteRequest {
                    selector: Selector::Id { id: spawned.id },
                    signal: "SIGTERM".to_string(),
                    grace_secs: 5,
                },
            },
        )
        .await;
    let RpcResponse::Deleted { response } = deleted.response else {
        panic!("expected delete response");
    };

    assert_eq!(response.sessions.len(), 1);
    assert_eq!(response.sessions[0].state, SessionState::Terminated);
    assert_eq!(response.sessions[0].exit_code, Some(143));
    assert!(response.sessions[0].terminated_at.is_some());
}

#[tokio::test]
pub(crate) async fn delete_unknown_id_uses_session_noun() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let id = uuid::Uuid::nil();

    let deleted = daemon
        .state
        .handle(
            local_context(),
            SessionRpc::Delete {
                request: DeleteRequest {
                    selector: Selector::Id { id },
                    signal: "SIGTERM".to_string(),
                    grace_secs: 5,
                },
            },
        )
        .await;
    let RpcResponse::Error { message } = deleted.response else {
        panic!("expected delete error response");
    };

    assert_eq!(message, format!("unknown session: {id}"));
}

#[tokio::test]
pub(crate) async fn label_empty_selector_uses_session_noun() {
    let daemon = TestDaemon::new(LOCAL_UID).await;

    let labeled = daemon
        .state
        .handle(
            local_context(),
            SessionRpc::Label {
                request: LabelRequest {
                    selector: Selector::All,
                    mutation: LabelMutation::Set(Label {
                        key: "scope".to_string(),
                        value: "alpha".to_string(),
                    }),
                },
            },
        )
        .await;
    let RpcResponse::Error { message } = labeled.response else {
        panic!("expected label error response");
    };

    assert_eq!(message, "selector matched no sessions: all");
}
