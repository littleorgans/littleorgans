use std::path::PathBuf;

use lilo_db::test_support::TestDb;
use lilo_rm_core::{
    HeadlessSpawnTarget, IsolationPolicy, RuntimeKind as RuntimeRuntimeKind, SpawnTarget,
};
use lilo_session_core::{Namespace, RuntimeKind};

use super::*;
use crate::test_support::{ATTACHMENT_VALUE_SENTINEL_41, OrPanic as _, launch_attachment_fixture};

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn intent_repository_inserts_and_lists_pending() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let intent = test_intent();

    store
        .insert_pending_spawn_intent(&intent)
        .await
        .expect("insert pending intent");

    let pending = store
        .list_pending_spawn_intents()
        .await
        .expect("list pending intents");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].session_id, intent.session_id);
    assert_eq!(pending[0].status, SpawnIntentStatus::Pending);
    assert_eq!(pending[0].spawn_request, intent.spawn_request);
    assert_eq!(pending[0].session_draft, intent.session_draft);
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn intent_repository_resolves_pending_intent() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let intent = test_intent();
    store
        .insert_pending_spawn_intent(&intent)
        .await
        .expect("insert pending intent");

    store
        .resolve_spawn_intent(intent.session_id)
        .await
        .expect("resolve intent");

    let pending = store
        .list_pending_spawn_intents()
        .await
        .expect("list pending intents");
    assert!(pending.is_empty());
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn intent_repository_aborts_pending_intent() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let intent = test_intent();
    store
        .insert_pending_spawn_intent(&intent)
        .await
        .expect("insert pending intent");

    store
        .abort_spawn_intent(intent.session_id, "runtime spawn failed")
        .await
        .expect("abort intent");

    let pending = store
        .list_pending_spawn_intents()
        .await
        .expect("list pending intents");
    assert!(pending.is_empty());
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn old_spawn_request_json_without_attachment_decodes_as_none() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let intent = test_intent();
    let request_json = old_request_json(intent.session_id);
    assert!(request_json.get("launch_attachment").is_none());
    insert_raw_intent(&store, &intent, request_json).await;

    let pending = store
        .list_pending_spawn_intents()
        .await
        .expect("old pending intent decodes");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].spawn_request, intent.spawn_request);
    assert_eq!(pending[0].spawn_request.launch_attachment, None);
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn attached_request_round_trips_and_status_updates_retain_json() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let resolved = attached_intent();
    store
        .insert_pending_spawn_intent(&resolved)
        .await
        .expect("insert attached pending intent");

    let pending = store
        .list_pending_spawn_intents()
        .await
        .expect("list attached pending intent");
    assert_eq!(pending[0].spawn_request, resolved.spawn_request);
    store
        .resolve_spawn_intent(resolved.session_id)
        .await
        .expect("resolve attached intent");
    assert_stored_request_eq(&store, &resolved).await;

    let aborted = attached_intent();
    store
        .insert_pending_spawn_intent(&aborted)
        .await
        .expect("insert second attached pending intent");
    store
        .abort_spawn_intent(aborted.session_id, "forced abort")
        .await
        .expect("abort attached intent");
    assert_stored_request_eq(&store, &aborted).await;
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn malformed_present_attachment_fails_pending_list_without_value_disclosure() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let intent = test_intent();
    let mut request_json = old_request_json(intent.session_id);
    request_json["launch_attachment"] = serde_json::json!({
        "kind": "issue41.test",
        "value": { "secret": ATTACHMENT_VALUE_SENTINEL_41 }
    });
    insert_raw_intent(&store, &intent, request_json).await;

    let error = store
        .list_pending_spawn_intents()
        .await
        .expect_err("malformed present attachment should fail pending list");
    assert!(matches!(error, SpawnIntentError::Json(_)), "{error:?}");
    let rendered = error.to_string();
    assert!(rendered.contains("missing field `version`"), "{rendered}");
    assert!(!rendered.contains(ATTACHMENT_VALUE_SENTINEL_41));
    testdb.cleanup().await.or_panic("test db cleans up");
}

fn test_intent() -> PendingSpawnIntent {
    test_intent_with_id(SessionId::from_uuid(uuid::Uuid::now_v7()))
}

fn attached_intent() -> PendingSpawnIntent {
    let mut intent = test_intent();
    intent.spawn_request.launch_attachment = Some(launch_attachment_fixture());
    intent
}

fn test_intent_with_id(id: SessionId) -> PendingSpawnIntent {
    PendingSpawnIntent::new(
        IntentId::from_uuid(uuid::Uuid::now_v7()),
        RuntimeSpawnRequest {
            session_id: id,
            runtime: RuntimeRuntimeKind::Claude,
            isolation: IsolationPolicy::Host,
            image: None,
            env: Vec::new(),
            mounts: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
            force: false,
            shell_resume: None,
            launch_attachment: None,
        },
        SessionDraft::new(&test_session(id)),
    )
}

fn test_session(id: SessionId) -> Session {
    let now = Utc::now();
    Session {
        id,
        runtime: RuntimeKind::Claude,
        role: "worker".to_owned(),
        workspace: "default".to_owned(),
        namespace: Namespace::default(),
        dir: PathBuf::from("/tmp"),
        labels: Vec::new(),
        state: SessionState::Running,
        runtime_pid: 1,
        runtime_session: None,
        transcript_path: None,
        tmux_pane: None,
        agent_config: None,
        created_at: now,
        started_at: now,
        terminated_at: None,
        exit_code: None,
        updated_at: now,
    }
}

fn old_request_json(session_id: SessionId) -> serde_json::Value {
    serde_json::json!({
        "session_id": session_id,
        "runtime": "claude",
        "isolation": { "type": "host" },
        "env": [],
        "cwd": "/tmp",
        "target": { "type": "headless", "payload": {} }
    })
}

async fn insert_raw_intent(
    store: &SessionStore,
    intent: &PendingSpawnIntent,
    spawn_request_json: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO session_spawn_intents
            (session_id, operation_id, status, spawn_request_json, session_draft_json,
             created_at, updated_at, resolved_at, aborted_reason)
         VALUES ($1, $2, $3, $4, $5, $6, $7, NULL, NULL)",
    )
    .bind(intent.session_id.to_string())
    .bind(intent.operation_id.to_string())
    .bind(SpawnIntentStatus::Pending.as_str())
    .bind(spawn_request_json.to_string())
    .bind(serde_json::to_string(&intent.session_draft).expect("serialize session draft"))
    .bind(intent.created_at)
    .bind(intent.created_at)
    .execute(store.pool())
    .await
    .or_panic("insert raw pending intent");
}

async fn assert_stored_request_eq(store: &SessionStore, intent: &PendingSpawnIntent) {
    let stored: String = sqlx::query_scalar(
        "SELECT spawn_request_json FROM session_spawn_intents WHERE session_id = $1",
    )
    .bind(intent.session_id.to_string())
    .fetch_one(store.pool())
    .await
    .or_panic("read retained spawn request JSON");
    let stored: RuntimeSpawnRequest =
        serde_json::from_str(&stored).expect("decode retained spawn request JSON");
    assert_eq!(stored, intent.spawn_request);
}
