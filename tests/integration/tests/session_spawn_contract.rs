use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::Arc;

use anyhow::{Context, Result};
use lilo_db::{begin_immediate_tx, finish_immediate_tx};
use lilo_integration_tests::{
    IntegrationFixture, count_rows, draft_session, event_log_line_count, fixed_uuid, running_event,
    running_lifecycle, runtime_config, runtime_request,
};
use lilo_runtime_daemon::{RuntimeService, RuntimeServiceContext};
use lilo_runtime_store::LifecycleStore;
use lilo_session_store::{PendingSpawnIntent, SessionDraft, SqliteStore};
use lilo_test_support::{LiloDaemon, assert_success, fake_runtime_path, stdout};
use uuid::Uuid;

#[tokio::test]
async fn lilo_session_user_verbs_route_through_session_spawn() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let runtime_path = fake_runtime_path("claude")?;
    let mut daemon = LiloDaemon::start(
        lilo_home(&fixture)?,
        fixture.paths.run_root().join("lilod.sock"),
        Some(runtime_path.path()),
    )?;
    let workspace = fixture.paths.tmp_root().join("workspace");
    fs::create_dir_all(&workspace)?;

    let run = daemon
        .command(["run", "claude", "--role", "worker", "--dir"])
        .arg(&workspace)
        .args(["--target", "headless", "--detach"])
        .output()
        .context("lilo run executes")?;
    assert_success("lilo run", &run);
    let run_id = stdout_session_id(&run)?;

    let create = daemon
        .command(["create", "session", "claude", "--role", "worker", "--dir"])
        .arg(&workspace)
        .output()
        .context("lilo create session executes")?;
    assert_success("lilo create session", &create);
    let created_id = stdout_session_id(&create)?;

    for session_id in [run_id, created_id] {
        assert_eq!(session_count(&fixture, session_id).await?, 1);
        assert_eq!(resolved_count(&fixture, session_id).await?, 1);
        assert_eq!(pending_count(&fixture, session_id).await?, 0);
        assert!(allowed_spawn_audit_count(&fixture, session_id).await? >= 1);
    }

    let raw_runtime_id = fixed_uuid(30);
    LifecycleStore::open(&fixture.db)
        .insert_forking(&lilo_rm_core::Lifecycle::forking(
            raw_runtime_id,
            lilo_rm_core::RuntimeKind::Claude,
        ))
        .await?;

    let get = daemon
        .command(["get", "session", "--json"])
        .output()
        .context("lilo get session executes")?;
    assert_success("lilo get session --json", &get);
    let listed_ids = listed_session_ids(&get)?;
    assert_eq!(listed_ids.len(), 2, "stdout: {}", stdout(&get));
    assert!(listed_ids.contains(&run_id));
    assert!(listed_ids.contains(&created_id));
    assert!(!listed_ids.contains(&raw_runtime_id));

    daemon.stop();
    Ok(())
}

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
    let runtime_path = fake_runtime_path("claude")?;
    let mut daemon = LiloDaemon::start(
        lilo_home(&fixture)?,
        fixture.paths.run_root().join("lilod.sock"),
        Some(runtime_path.path()),
    )?;
    let workspace = fixture.paths.tmp_root().join("raw-runtime-workspace");
    fs::create_dir_all(&workspace)?;
    let session_id = fixed_uuid(10);

    let spawn = daemon
        .command(["runtime", "spawn", "--runtime", "claude", "--session-id"])
        .arg(session_id.to_string())
        .args(["--target", "headless", "--cwd"])
        .arg(&workspace)
        .output()
        .context("lilo runtime spawn executes")?;
    assert_success("lilo runtime spawn", &spawn);

    let status = daemon
        .command(["runtime", "status", "--session-id"])
        .arg(session_id.to_string())
        .output()
        .context("lilo runtime status executes")?;
    assert_success("lilo runtime status", &status);
    assert_eq!(runtime_status_session_ids(&status)?, vec![session_id]);

    let events = daemon
        .command(["runtime", "events"])
        .output()
        .context("lilo runtime events executes")?;
    assert_success("lilo runtime events", &events);
    assert!(runtime_event_session_ids(&events)?.contains(&session_id));

    let get = daemon
        .command(["get", "session", "--json"])
        .output()
        .context("lilo get session executes")?;
    assert_success("lilo get session --json", &get);
    assert!(!listed_session_ids(&get)?.contains(&session_id));

    assert!(allowed_spawn_audit_count(&fixture, session_id).await? >= 1);
    assert_eq!(lifecycle_count(&fixture, session_id).await?, 1);
    assert_eq!(pending_count(&fixture, session_id).await?, 0);
    assert_eq!(session_count(&fixture, session_id).await?, 0);

    let kill = daemon
        .command(["runtime", "kill"])
        .arg(session_id.to_string())
        .output()
        .context("lilo runtime kill executes")?;
    assert_success("lilo runtime kill", &kill);

    daemon.stop();
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

async fn allowed_spawn_audit_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.identity_pool(),
        "SELECT COUNT(*) FROM identity_audit
         WHERE session_ref = ? AND action = '\"spawn\"' AND decision = '{\"kind\":\"allow\"}'",
        &session_id.to_string(),
    )
    .await
}

fn lilo_home(fixture: &IntegrationFixture) -> Result<PathBuf> {
    fixture
        .paths
        .data_root()
        .parent()
        .map(Path::to_path_buf)
        .context("fixture data root has a home parent")
}

fn stdout_session_id(output: &Output) -> Result<Uuid> {
    stdout(output)
        .split_whitespace()
        .next()
        .context("session output has an id")?
        .parse()
        .context("session id parses")
}

fn listed_session_ids(output: &Output) -> Result<Vec<Uuid>> {
    let sessions: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("session list JSON parses")?;
    let array = sessions
        .as_array()
        .context("session list JSON is an array")?;
    array
        .iter()
        .map(|session| uuid_field(session, "id"))
        .collect()
}

fn runtime_status_session_ids(output: &Output) -> Result<Vec<Uuid>> {
    let lifecycles: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("runtime status JSON parses")?;
    let array = lifecycles
        .as_array()
        .context("runtime status JSON is an array")?;
    array
        .iter()
        .map(|lifecycle| uuid_field(lifecycle, "session_id"))
        .collect()
}

fn runtime_event_session_ids(output: &Output) -> Result<Vec<Uuid>> {
    let payload: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("runtime events JSON parses")?;
    let array = payload
        .get("events")
        .and_then(serde_json::Value::as_array)
        .context("runtime events JSON has an events array")?;
    array
        .iter()
        .map(|event| {
            let payload = event
                .get("payload")
                .context("runtime event JSON has a payload")?;
            uuid_field(payload, "session_id")
        })
        .collect()
}

fn uuid_field(value: &serde_json::Value, field: &'static str) -> Result<Uuid> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .with_context(|| format!("JSON object has {field}"))?
        .parse()
        .with_context(|| format!("JSON {field} parses"))
}
