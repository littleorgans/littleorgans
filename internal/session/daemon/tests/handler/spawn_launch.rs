use crate::common::{
    LOCAL_UID, TestDaemon, headless_spawn_request, local_context, spawn_request, spawn_test_session,
};
use lilo_im_core::{Action, AuditDecision};
use lilo_session_core::{Namespace, RpcResponse, RuntimeKind, SessionRpc};

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
pub(crate) async fn spawn_launch_uses_runtime_service_without_driver_fallback() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();

    let spawned = daemon
        .state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(headless_spawn_request(
                    "pm",
                    daemon.dir.path().display().to_string(),
                )),
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
    daemon.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
pub(crate) async fn spawn_launch_cwd_is_request_workspace() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let session = spawn_test_session(&daemon, &local_context(), "pm").await;

    assert_eq!(session.namespace, Namespace::default());
    assert_eq!(session.workspace, daemon.dir.path().display().to_string());
    assert_eq!(session.dir, daemon.dir.path());
    assert!(session.runtime_pid > 0);
    daemon.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
pub(crate) async fn composed_spawn_writes_one_session_door_audit_row() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let state = daemon.in_process_state();
    let context = local_context();
    let principal = context.principal.clone();

    let spawned = state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(headless_spawn_request(
                    "pm",
                    daemon.dir.path().display().to_string(),
                )),
            },
        )
        .await;
    let RpcResponse::Spawned { response } = spawned.response else {
        panic!("expected spawn response");
    };

    let rows = daemon.audit_rows().await;
    let spawn_rows = rows
        .iter()
        .filter(|row| row.principal == principal && row.action == Action::Spawn)
        .collect::<Vec<_>>();
    assert_eq!(spawn_rows.len(), 1);
    assert_eq!(spawn_rows[0].decision, AuditDecision::Allow);
    assert_eq!(spawn_rows[0].resource.session_id, Some(response.session.id));
    daemon.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
pub(crate) async fn spawn_rejects_invalid_target_before_transaction_a() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let response = daemon
        .state
        .handle(
            local_context(),
            SessionRpc::Spawn {
                request: Box::new(spawn_request(
                    "pm",
                    daemon.dir.path().display().to_string(),
                    "invalid-target",
                )),
            },
        )
        .await;
    let RpcResponse::Error { message } = response.response else {
        panic!("expected invalid target error");
    };
    assert!(
        message.contains("invalid spawn target invalid-target"),
        "{message}"
    );
    let intent_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session_spawn_intents")
        .fetch_one(daemon.testdb.db().pool())
        .await
        .expect("spawn intent count reads");
    assert_eq!(intent_count, 0);
    daemon.cleanup().await;
}
