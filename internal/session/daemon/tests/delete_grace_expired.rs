mod common;

use common::{LOCAL_UID, TestDaemon, local_context, spawn_test_session};
use lilo_session_core::{DeleteRequest, RpcRequest, RpcResponse, Selector};

#[tokio::test]
async fn delete_reports_grace_expired_when_driver_returns_no_exit() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let session = spawn_test_session(&daemon, &context, "engineer").await;
    daemon.driver.set_terminate_exit(None);

    let deleted = daemon
        .state
        .handle(
            context,
            RpcRequest::Delete {
                request: DeleteRequest {
                    selector: Selector::Id { id: session.id },
                    signal: "SIGTERM".to_string(),
                    grace_secs: 3,
                },
            },
        )
        .await;
    let RpcResponse::Deleted { response } = deleted.response else {
        panic!("expected delete response");
    };

    assert!(response.sessions.is_empty());
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].target, session.id.to_string());
    assert_eq!(
        response.errors[0].message,
        format!(
            "runtime did not terminate within 3 grace seconds: {}",
            session.id
        )
    );
    assert!(!response.errors[0].message.contains("owned"));
}
