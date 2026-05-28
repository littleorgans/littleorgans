use chrono::{DateTime, TimeZone, Utc};
use lilo_rm_core::{
    IsolationPolicy, IsolationProfile, Lifecycle, LifecycleState, LostEvidence, RuntimeKind,
    ShimReady, StatusFilter,
};
use tempfile::TempDir;
use uuid::Uuid;

use super::LifecycleStore;

#[tokio::test]
async fn persists_lifecycle_transitions() {
    let (_temp, store) = lifecycle_store().await;
    let session_id = Uuid::now_v7();
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
}

#[tokio::test]
async fn tmux_pane_round_trips_through_sqlite() {
    let (_temp, store) = lifecycle_store().await;
    let session_id = Uuid::now_v7();
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
}

#[tokio::test]
async fn isolation_policy_round_trips_through_sqlite() {
    let (_temp, store) = lifecycle_store().await;
    let session_id = Uuid::now_v7();
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    lifecycle.isolation = IsolationPolicy::Docker(IsolationProfile {
        name: Some("locked".to_owned()),
    });

    store.insert_forking(&lifecycle).await.expect("insert");

    let restored = store.get(session_id).await.expect("get").expect("row");
    assert_eq!(restored.isolation, lifecycle.isolation);
}

#[tokio::test]
async fn lists_lifecycles_with_composed_status_filters() {
    let (_temp, store) = lifecycle_store().await;
    let old_claude = Uuid::now_v7();
    let wanted = Uuid::now_v7();
    let wrong_state = Uuid::now_v7();

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
}

#[tokio::test]
async fn reports_counts_migrations_probe_sweep_and_recent_lost() {
    let (_temp, store) = lifecycle_store().await;
    let session_id = Uuid::now_v7();
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    store.insert_forking(&lifecycle).await.expect("insert");
    lifecycle.mark_lost(LostEvidence::PidNotAlive);
    store.update_lifecycle(&lifecycle).await.expect("lost");

    let swept_at = Utc::now();
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
}

#[tokio::test]
async fn migration_is_idempotent() {
    let temp = TempDir::new().expect("temp dir");
    let path = temp.path().join("rtm.sqlite");

    LifecycleStore::path_open(path.clone())
        .await
        .expect("first open");
    LifecycleStore::path_open(path).await.expect("second open");
}

async fn insert_running(
    store: &LifecycleStore,
    session_id: Uuid,
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

async fn insert_lost(store: &LifecycleStore, session_id: Uuid, runtime: RuntimeKind) {
    let mut lifecycle = Lifecycle::forking(session_id, runtime);
    store.insert_forking(&lifecycle).await.expect("insert");
    assert!(lifecycle.mark_lost(LostEvidence::PidNotAlive));
    store.update_lifecycle(&lifecycle).await.expect("update");
}

async fn set_updated_at(store: &LifecycleStore, session_id: Uuid, updated_at: DateTime<Utc>) {
    sqlx::query("UPDATE runtime_lifecycle SET updated_at = ? WHERE session_id = ?")
        .bind(updated_at.to_rfc3339())
        .bind(session_id.to_string())
        .execute(store.pool())
        .await
        .expect("set updated_at");
}

async fn lifecycle_store() -> (TempDir, LifecycleStore) {
    let temp = TempDir::new().expect("temp dir");
    let store = LifecycleStore::path_open(temp.path().join("rtm.sqlite"))
        .await
        .expect("store");

    (temp, store)
}

fn test_time(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + seconds, 0).unwrap()
}
