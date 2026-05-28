use std::sync::Arc;

use anyhow::Result;
use lilo_db::{begin_immediate_tx, finish_immediate_tx};
use lilo_integration_tests::{
    IntegrationFixture, count_rows, draft_session, event_log_line_count, fixed_uuid, running_event,
    running_lifecycle, runtime_config, runtime_request,
};
use lilo_runtime_daemon::{RuntimeService, RuntimeServiceContext};
use lilo_runtime_store::LifecycleStore;
use lilo_session_store::{PendingSpawnIntent, SessionDraft, SqliteStore};
use uuid::Uuid;

#[tokio::test]
async fn session_spawn_persists_fixed_order_across_two_transactions() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let session_store = SqliteStore::open(&fixture.db);
    let lifecycle_store = LifecycleStore::open(&fixture.db);
    let session_id = fixed_uuid(1);
    let mut observed = Vec::new();
    let intent = PendingSpawnIntent::new(
        fixed_uuid(2),
        runtime_request(session_id),
        SessionDraft::new(&draft_session(session_id)),
    );

    let mut tx_a = fixture.db.session_pool().acquire().await?;
    begin_immediate_tx(&mut tx_a, "integration tx a").await?;
    let tx_a_result = async {
        insert_audit(&mut *tx_a, "audit-session-spawn", session_id).await?;
        observed.push("identity-audit");
        session_store
            .insert_pending_spawn_intent_in(&mut tx_a, &intent)
            .await?;
        observed.push("intent-pending");
        lifecycle_store
            .insert_forking_in(
                &mut tx_a,
                &lilo_rm_core::Lifecycle::forking(session_id, lilo_rm_core::RuntimeKind::Claude),
            )
            .await?;
        observed.push("runtime-forking");
        Result::<()>::Ok(())
    }
    .await;
    finish_immediate_tx(&mut tx_a, tx_a_result, "integration tx a").await?;

    assert_eq!(pending_count(&fixture, session_id).await?, 1);
    assert_eq!(resolved_count(&fixture, session_id).await?, 0);
    assert_eq!(session_count(&fixture, session_id).await?, 0);

    let running = running_lifecycle(session_id);
    lifecycle_store.update_lifecycle(&running).await?;
    observed.push("runtime-kqueue-ready");

    let mut tx_b = fixture.db.session_pool().acquire().await?;
    begin_immediate_tx(&mut tx_b, "integration tx b").await?;
    let tx_b_result = async {
        let session = intent
            .session_draft
            .running_session(&running, None, chrono::Utc::now())?;
        session_store.insert_session_in(&mut tx_b, &session).await?;
        observed.push("session-record");
        session_store
            .resolve_spawn_intent_in(&mut tx_b, session_id)
            .await?;
        observed.push("intent-resolved");
        Result::<()>::Ok(())
    }
    .await;
    finish_immediate_tx(&mut tx_b, tx_b_result, "integration tx b").await?;

    assert_eq!(
        observed,
        [
            "identity-audit",
            "intent-pending",
            "runtime-forking",
            "runtime-kqueue-ready",
            "session-record",
            "intent-resolved",
        ]
    );
    assert_eq!(pending_count(&fixture, session_id).await?, 0);
    assert_eq!(resolved_count(&fixture, session_id).await?, 1);
    assert_eq!(session_count(&fixture, session_id).await?, 1);
    Ok(())
}

#[tokio::test]
async fn raw_runtime_spawn_keeps_session_tables_empty() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let lifecycle_store = LifecycleStore::open(&fixture.db);
    let session_id = fixed_uuid(10);

    insert_audit(
        fixture.db.identity_pool(),
        "audit-raw-runtime-spawn",
        session_id,
    )
    .await?;
    lifecycle_store
        .insert_forking(&lilo_rm_core::Lifecycle::forking(
            session_id,
            lilo_rm_core::RuntimeKind::Claude,
        ))
        .await?;

    assert_eq!(audit_count(&fixture, session_id).await?, 1);
    assert_eq!(lifecycle_count(&fixture, session_id).await?, 1);
    assert_eq!(pending_count(&fixture, session_id).await?, 0);
    assert_eq!(session_count(&fixture, session_id).await?, 0);
    Ok(())
}

#[tokio::test]
async fn startup_reconcile_appends_d9_only_after_tx_b_commit() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let runtime = Arc::new(
        RuntimeService::build(RuntimeServiceContext::new(
            runtime_config(&fixture.paths),
            fixture.db.clone(),
        ))
        .await?,
    );
    let session_store = SqliteStore::open(&fixture.db);
    let lifecycle_store = LifecycleStore::open(&fixture.db);
    let session_id = fixed_uuid(20);
    let intent = PendingSpawnIntent::new(
        fixed_uuid(21),
        runtime_request(session_id),
        SessionDraft::new(&draft_session(session_id)),
    );

    session_store.insert_pending_spawn_intent(&intent).await?;
    lifecycle_store
        .insert_forking(&lilo_rm_core::Lifecycle::forking(
            session_id,
            lilo_rm_core::RuntimeKind::Claude,
        ))
        .await?;
    lifecycle_store
        .update_lifecycle(&running_lifecycle(session_id))
        .await?;
    sqlx::query(
        "CREATE TRIGGER fail_resolve_before_event
         BEFORE UPDATE OF status ON session_spawn_intents
         WHEN NEW.status = 'resolved'
         BEGIN
           SELECT RAISE(ABORT, 'forced tx b failure');
         END",
    )
    .execute(fixture.db.session_pool())
    .await?;

    let service = lilo_session_daemon::SessionService::build(
        lilo_session_daemon::SessionServiceContext::new(
            fixture.paths.clone(),
            fixture.db.clone(),
            Arc::clone(&runtime),
        ),
    )?;
    assert!(service.reconcile_pending_spawn_intents().await.is_err());
    assert_eq!(event_log_line_count(&fixture.paths)?, 0);
    assert_eq!(pending_count(&fixture, session_id).await?, 1);
    assert_eq!(session_count(&fixture, session_id).await?, 0);

    sqlx::query("DROP TRIGGER fail_resolve_before_event")
        .execute(fixture.db.session_pool())
        .await?;
    service.reconcile_pending_spawn_intents().await?;

    assert_eq!(resolved_count(&fixture, session_id).await?, 1);
    assert_eq!(session_count(&fixture, session_id).await?, 1);
    assert_eq!(event_log_line_count(&fixture.paths)?, 1);

    runtime.append_event(running_event(session_id)).await?;
    assert_eq!(event_log_line_count(&fixture.paths)?, 1);
    runtime.shutdown().await?;
    Ok(())
}

async fn insert_audit<'e, E>(executor: E, id: &str, session_id: Uuid) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query(
        "INSERT INTO identity_audit
         (id, timestamp, principal, action, resource, decision, session_ref)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind("2026-05-28T00:00:00Z")
    .bind("local:0")
    .bind("runtime_spawn")
    .bind(format!("runtime:{session_id}"))
    .bind("allow")
    .bind(session_id.to_string())
    .execute(executor)
    .await?;
    Ok(())
}

async fn audit_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.identity_pool(),
        "SELECT COUNT(*) FROM identity_audit WHERE session_ref = ?",
        &session_id.to_string(),
    )
    .await
}

async fn lifecycle_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.runtime_pool(),
        "SELECT COUNT(*) FROM runtime_lifecycle WHERE session_id = ?",
        &session_id.to_string(),
    )
    .await
}

async fn pending_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.session_pool(),
        "SELECT COUNT(*) FROM session_spawn_intents WHERE session_id = ? AND status = 'pending'",
        &session_id.to_string(),
    )
    .await
}

async fn resolved_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.session_pool(),
        "SELECT COUNT(*) FROM session_spawn_intents WHERE session_id = ? AND status = 'resolved'",
        &session_id.to_string(),
    )
    .await
}

async fn session_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.session_pool(),
        "SELECT COUNT(*) FROM session_sessions WHERE id = ?",
        &session_id.to_string(),
    )
    .await
}
