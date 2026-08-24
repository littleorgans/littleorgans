use chrono::{DateTime, TimeZone, Utc};
use lilo_common::id::SessionId;
use lilo_db::test_support::{TestDb, now_micros};
use lilo_db::{DbConfig, LiloDb};
use lilo_rm_core::{
    IsolationPolicy, IsolationProfile, Lifecycle, LifecycleState, LostEvidence, RuntimeKind,
    ShimReady, StatusFilter,
};

use super::LifecycleStore;

const REQUIRES_POSTGRES: &str =
    "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all";

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn persists_lifecycle_transitions() {
    let (testdb, store) = lifecycle_store().await;
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);

    store.insert_forking(&lifecycle).await.expect("insert");
    lifecycle.state = LifecycleState::Lost(LostEvidence::PidNotAlive);
    store.update_lifecycle(&lifecycle).await.expect("update");

    let restored = store.get(session_id).await.expect("get").expect("row");
    assert_eq!(
        restored.state,
        LifecycleState::Lost(LostEvidence::PidNotAlive)
    );
    assert_eq!(store.running().await.expect("running").len(), 0);
    testdb.cleanup().await.expect("cleanup");
}

/// `runtime_lifecycle.lost_evidence` keeps its own text vocabulary, so a new
/// `LostEvidence` variant reaches this codec's fallback arm and stops persisting
/// unless it is given an encoding here too. Iterating `ALL` is what surfaces
/// that, rather than a hand picked variant.
#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn lost_lifecycles_round_trip_for_every_evidence_variant() {
    let (testdb, store) = lifecycle_store().await;

    for evidence in LostEvidence::ALL {
        let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
        let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
        store.insert_forking(&lifecycle).await.expect("insert");
        lifecycle.state = LifecycleState::Lost(evidence);
        store.update_lifecycle(&lifecycle).await.expect("update");

        let restored = store.get(session_id).await.expect("get").expect("row");
        assert_eq!(restored.state, LifecycleState::Lost(evidence));
    }

    testdb.cleanup().await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn tmux_pane_round_trips() {
    let (testdb, store) = lifecycle_store().await;
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    lifecycle.mark_running(ShimReady {
        session_id,
        shim_pid: 10,
        runtime_pid: 20,
        start_time: Utc::now(),
        tmux_pane: Some("test:0.1".parse().expect("tmux pane")),
    });

    store
        .insert_forking(&Lifecycle::forking(session_id, RuntimeKind::Claude))
        .await
        .expect("insert");
    store.update_lifecycle(&lifecycle).await.expect("update");

    let restored = store.get(session_id).await.expect("get").expect("row");
    assert_eq!(restored.tmux_pane, lifecycle.tmux_pane);
    testdb.cleanup().await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn isolation_policy_round_trips() {
    let (testdb, store) = lifecycle_store().await;
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    lifecycle.isolation = IsolationPolicy::Docker(IsolationProfile {
        name: Some("locked".to_owned()),
    });

    store.insert_forking(&lifecycle).await.expect("insert");

    let restored = store.get(session_id).await.expect("get").expect("row");
    assert_eq!(restored.isolation, lifecycle.isolation);
    testdb.cleanup().await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn lists_lifecycles_with_composed_status_filters() {
    let (testdb, store) = lifecycle_store().await;
    let old_claude = SessionId::from_uuid(uuid::Uuid::now_v7());
    let wanted = SessionId::from_uuid(uuid::Uuid::now_v7());
    let wrong_state = SessionId::from_uuid(uuid::Uuid::now_v7());

    insert_running(&store, old_claude, RuntimeKind::Claude, 10).await;
    insert_running(&store, wanted, RuntimeKind::Codex, 20).await;
    insert_lost(&store, wrong_state, RuntimeKind::Codex).await;
    set_updated_at(&store, old_claude, test_time(0)).await;
    set_updated_at(&store, wanted, test_time(10)).await;
    set_updated_at(&store, wrong_state, test_time(20)).await;

    let rows = store
        .list(&StatusFilter {
            session_id: Some(old_claude),
            session_ids: vec![wanted, wrong_state],
            updated_since: Some(test_time(10)),
            runtime: Some("codex".to_owned()),
            state: Some("running".to_owned()),
        })
        .await
        .expect("filtered lifecycles");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].session_id, wanted);
    testdb.cleanup().await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn reports_counts_migrations_probe_sweep_and_recent_lost() {
    let (testdb, store) = lifecycle_store().await;
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    store.insert_forking(&lifecycle).await.expect("insert");
    lifecycle.mark_lost(LostEvidence::PidNotAlive);
    store.update_lifecycle(&lifecycle).await.expect("lost");

    let swept_at = now_micros();
    store
        .record_probe_sweep(swept_at)
        .await
        .expect("record sweep");

    let counts = store.lifecycle_counts().await.expect("counts");
    assert_eq!(counts.lost, 1);
    let migrations = store.migration_state().await.expect("migrations");
    assert_eq!(migrations.applied, migrations.total);
    assert_eq!(migrations.total, 1);
    assert_eq!(
        store.last_probe_sweep().await.expect("last sweep"),
        Some(swept_at)
    );
    let recent = store
        .recent_lost_since(Utc::now() - chrono::Duration::hours(1))
        .await
        .expect("recent lost");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].session_id, session_id);
    assert_eq!(recent[0].evidence, LostEvidence::PidNotAlive);
    testdb.cleanup().await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn migration_is_idempotent() {
    let testdb = TestDb::create().await.expect("create test db");
    // TestDb::create already ran migrations once; reopening the same database
    // re-runs the migrator, which must be a no-op.
    LiloDb::open_postgres(DbConfig::from_url(testdb.database_url().to_owned()))
        .await
        .expect("second open");
    testdb.cleanup().await.expect("cleanup");
}

async fn insert_running(
    store: &LifecycleStore,
    session_id: SessionId,
    runtime: RuntimeKind,
    runtime_pid: u32,
) {
    let mut lifecycle = Lifecycle::forking(session_id, runtime);
    store.insert_forking(&lifecycle).await.expect("insert");
    assert!(lifecycle.mark_running(ShimReady {
        session_id,
        shim_pid: runtime_pid - 1,
        runtime_pid,
        start_time: test_time(0),
        tmux_pane: None,
    }));
    store.update_lifecycle(&lifecycle).await.expect("update");
}

async fn insert_lost(store: &LifecycleStore, session_id: SessionId, runtime: RuntimeKind) {
    let mut lifecycle = Lifecycle::forking(session_id, runtime);
    store.insert_forking(&lifecycle).await.expect("insert");
    assert!(lifecycle.mark_lost(LostEvidence::PidNotAlive));
    store.update_lifecycle(&lifecycle).await.expect("update");
}

async fn set_updated_at(store: &LifecycleStore, session_id: SessionId, updated_at: DateTime<Utc>) {
    sqlx::query("UPDATE runtime_lifecycle SET updated_at = $1 WHERE session_id = $2")
        .bind(updated_at)
        .bind(session_id.to_string())
        .execute(store.pool())
        .await
        .expect("set updated_at");
}

async fn lifecycle_store() -> (TestDb, LifecycleStore) {
    let testdb = TestDb::create().await.expect(REQUIRES_POSTGRES);
    let store = LifecycleStore::from_db(testdb.db());
    (testdb, store)
}

fn test_time(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + seconds, 0).unwrap()
}
