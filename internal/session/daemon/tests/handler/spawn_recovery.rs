use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use lilo_common::id::SessionId;
use lilo_db::LiloDb;
use lilo_rm_core::{
    EventBatch, EventsRequest, IsolationPolicy, Lifecycle, NudgeMode,
    RuntimeKind as RuntimeRuntimeKind, RuntimeSignal, ShimReady,
    SpawnRequest as RuntimeSpawnRequest, StatusFilter,
};
use lilo_runtime_store::LifecycleStore;
use lilo_session_core::{RpcResponse, RuntimeDoctorReport, SessionRpc};
use lilo_session_driver::{
    CaptureResult, ChildExit, NudgeResult, RuntimeError, RuntimePort, SpawnedProcess,
};

use crate::common::{LOCAL_UID, OrPanic as _, TestDaemon, local_context, spawn_request};

type PortFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + Send + 'a>>;

const TEST_RUNTIME_PID: u32 = 42_424;

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
pub(crate) async fn tx_b_failure_aborts_started_runtime_and_spawn_intent() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let runtime = Arc::new(FaultingRuntimePort::new(
        daemon.testdb.db().clone(),
        SpawnFault::FailTxBResolve,
    ));
    let state = daemon.state_with_runtime_port(Arc::clone(&runtime) as Arc<dyn RuntimePort>);

    let result = state
        .handle(
            local_context(),
            SessionRpc::Spawn {
                request: Box::new(spawn_request(
                    "pm",
                    daemon.dir.path().display().to_string(),
                    "headless",
                )),
            },
        )
        .await;

    let RpcResponse::Error { message } = result.response else {
        panic!("expected Tx-B failure response");
    };
    assert!(
        message.contains("forced Tx-B resolve failure"),
        "unexpected error: {message}"
    );
    let session_id = runtime.spawned_session_id();
    assert!(runtime.terminated(session_id));
    assert_eq!(
        spawn_intent_status(daemon.testdb.db(), session_id)
            .await
            .as_deref(),
        Some("aborted")
    );
    assert_no_lifecycle(daemon.testdb.db(), session_id).await;
    assert_eq!(session_row_count(daemon.testdb.db(), session_id).await, 0);
    daemon.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
pub(crate) async fn abort_spawn_intent_clears_forking_and_marks_intent_aborted() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let runtime = Arc::new(FaultingRuntimePort::new(
        daemon.testdb.db().clone(),
        SpawnFault::FailRuntimeSpawn,
    ));
    let state = daemon.state_with_runtime_port(Arc::clone(&runtime) as Arc<dyn RuntimePort>);

    let result = state
        .handle(
            local_context(),
            SessionRpc::Spawn {
                request: Box::new(spawn_request(
                    "pm",
                    daemon.dir.path().display().to_string(),
                    "headless",
                )),
            },
        )
        .await;

    let RpcResponse::Error { message } = result.response else {
        panic!("expected runtime spawn failure response");
    };
    assert!(
        message.contains("forced runtime spawn failure"),
        "unexpected error: {message}"
    );
    let session_id = runtime.spawned_session_id();
    assert_eq!(
        spawn_intent_status(daemon.testdb.db(), session_id)
            .await
            .as_deref(),
        Some("aborted")
    );
    assert_no_lifecycle(daemon.testdb.db(), session_id).await;
    assert_eq!(session_row_count(daemon.testdb.db(), session_id).await, 0);
    daemon.cleanup().await;
}

#[derive(Clone, Copy)]
enum SpawnFault {
    FailTxBResolve,
    FailRuntimeSpawn,
}

struct FaultingRuntimePort {
    db: LiloDb,
    fault: SpawnFault,
    spawned_session_id: Mutex<Option<SessionId>>,
    terminated_session_ids: Mutex<Vec<SessionId>>,
}

impl FaultingRuntimePort {
    fn new(db: LiloDb, fault: SpawnFault) -> Self {
        Self {
            db,
            fault,
            spawned_session_id: Mutex::new(None),
            terminated_session_ids: Mutex::new(Vec::new()),
        }
    }

    fn spawned_session_id(&self) -> SessionId {
        self.spawned_session_id
            .lock()
            .or_panic("spawned id lock succeeds")
            .or_panic("runtime spawn was attempted")
    }

    fn terminated(&self, session_id: SessionId) -> bool {
        self.terminated_session_ids
            .lock()
            .or_panic("terminated ids lock succeeds")
            .contains(&session_id)
    }

    async fn spawn_with_fault(
        &self,
        request: RuntimeSpawnRequest,
    ) -> Result<SpawnedProcess, RuntimeError> {
        let session_id = request.session_id;
        *self
            .spawned_session_id
            .lock()
            .or_panic("spawned id lock succeeds") = Some(session_id);
        match self.fault {
            SpawnFault::FailTxBResolve => {
                install_tx_b_resolve_failure(&self.db, session_id).await?;
                Ok(spawned_process(
                    session_id,
                    request.runtime,
                    request.isolation,
                ))
            }
            SpawnFault::FailRuntimeSpawn => {
                Err(RuntimeError::local("forced runtime spawn failure"))
            }
        }
    }
}

impl RuntimePort for FaultingRuntimePort {
    fn spawn(&self, request: RuntimeSpawnRequest) -> PortFuture<'_, SpawnedProcess> {
        Box::pin(async move { self.spawn_with_fault(request).await })
    }

    fn reap_exited(&self) -> PortFuture<'_, Vec<ChildExit>> {
        unsupported("reap_exited")
    }

    fn capture(
        &self,
        _session_id: SessionId,
        _scrollback_lines: Option<u32>,
    ) -> PortFuture<'_, CaptureResult> {
        unsupported("capture")
    }

    fn terminate(
        &self,
        session_id: SessionId,
        _signal: RuntimeSignal,
        _grace: Duration,
    ) -> PortFuture<'_, Option<ChildExit>> {
        Box::pin(async move {
            self.terminated_session_ids
                .lock()
                .or_panic("terminated ids lock succeeds")
                .push(session_id);
            Ok(Some(ChildExit {
                session_id,
                runtime_pid: TEST_RUNTIME_PID,
                exit_code: Some(143),
                transcript_path: None,
            }))
        })
    }

    fn nudge<'a>(
        &'a self,
        _session_id: SessionId,
        _content: &'a str,
        _mode: NudgeMode,
        _timeout_ms: Option<u64>,
    ) -> PortFuture<'a, NudgeResult> {
        unsupported("nudge")
    }

    fn status(&self, _filter: StatusFilter) -> PortFuture<'_, Vec<Lifecycle>> {
        unsupported("status")
    }

    fn poll_events(&self, _request: EventsRequest) -> PortFuture<'_, EventBatch> {
        unsupported("poll_events")
    }

    fn doctor(&self) -> PortFuture<'_, RuntimeDoctorReport> {
        unsupported("doctor")
    }

    fn terminate_all(&self) {}
}

fn unsupported<T: Send + 'static>(operation: &'static str) -> PortFuture<'static, T> {
    Box::pin(async move {
        Err(RuntimeError::local(format!(
            "unsupported driver operation {operation}; scheduled for WS5 test"
        )))
    })
}

fn spawned_process(
    session_id: SessionId,
    runtime: RuntimeRuntimeKind,
    isolation: IsolationPolicy,
) -> SpawnedProcess {
    let mut lifecycle = Lifecycle::forking(session_id, runtime);
    lifecycle.isolation = isolation;
    assert!(lifecycle.mark_running(ShimReady {
        session_id,
        shim_pid: TEST_RUNTIME_PID,
        runtime_pid: TEST_RUNTIME_PID,
        start_time: Utc::now(),
        tmux_pane: None,
    }));
    SpawnedProcess {
        lifecycle,
        runtime_pid: TEST_RUNTIME_PID,
        log_dir: None,
        stdout_path: None,
        stderr_path: None,
        tmux_pane: None,
    }
}

async fn install_tx_b_resolve_failure(
    db: &LiloDb,
    session_id: SessionId,
) -> Result<(), RuntimeError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ws5_forced_resolve_failures (
            session_id TEXT PRIMARY KEY NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .map_err(|error| RuntimeError::local(format!("failed to create Tx-B fault table: {error}")))?;
    sqlx::query(
        "CREATE OR REPLACE FUNCTION ws5_fail_spawn_intent_resolve()
         RETURNS trigger AS $$
         BEGIN
             IF NEW.status = 'resolved'
                AND EXISTS (
                    SELECT 1
                    FROM ws5_forced_resolve_failures
                    WHERE session_id = NEW.session_id
                )
             THEN
                 RAISE EXCEPTION 'forced Tx-B resolve failure';
             END IF;
             RETURN NEW;
         END;
         $$ LANGUAGE plpgsql",
    )
    .execute(db.pool())
    .await
    .map_err(|error| {
        RuntimeError::local(format!("failed to create Tx-B fault function: {error}"))
    })?;
    sqlx::query(
        "CREATE OR REPLACE TRIGGER ws5_fail_spawn_intent_resolve_trigger
         BEFORE UPDATE OF status ON session_spawn_intents
         FOR EACH ROW
         EXECUTE FUNCTION ws5_fail_spawn_intent_resolve()",
    )
    .execute(db.pool())
    .await
    .map_err(|error| {
        RuntimeError::local(format!("failed to create Tx-B fault trigger: {error}"))
    })?;
    sqlx::query("INSERT INTO ws5_forced_resolve_failures (session_id) VALUES ($1)")
        .bind(session_id.to_string())
        .execute(db.pool())
        .await
        .map_err(|error| RuntimeError::local(format!("failed to install Tx-B fault: {error}")))?;
    Ok(())
}

async fn spawn_intent_status(db: &LiloDb, session_id: SessionId) -> Option<String> {
    sqlx::query_scalar("SELECT status FROM session_spawn_intents WHERE session_id = $1")
        .bind(session_id.to_string())
        .fetch_optional(db.pool())
        .await
        .or_panic("spawn intent status query succeeds")
}

async fn session_row_count(db: &LiloDb, session_id: SessionId) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM session_sessions WHERE id = $1")
        .bind(session_id.to_string())
        .fetch_one(db.pool())
        .await
        .or_panic("session row count query succeeds")
}

async fn assert_no_lifecycle(db: &LiloDb, session_id: SessionId) {
    let lifecycle = LifecycleStore::from_db(db)
        .get(session_id)
        .await
        .or_panic("lifecycle query succeeds");
    assert!(
        lifecycle.is_none(),
        "expected no lifecycle for {session_id}"
    );
}
