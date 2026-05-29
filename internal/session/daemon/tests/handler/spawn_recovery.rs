use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use lilo_rm_core::{
    EventBatch, EventsRequest, IsolationPolicy, Lifecycle, RuntimeKind as RuntimeRuntimeKind,
    ShimReady, StatusFilter,
};
use lilo_runtime_store::LifecycleStore;
use lilo_session_core::{RpcResponse, RuntimeDoctorReport, RuntimeKind, SessionRpc};
use lilo_session_driver::{
    CaptureResult, ChildExit, NudgeResult, RuntimeError, RuntimeFault, RuntimePort, SpawnLaunch,
    SpawnedProcess,
};
use lilo_session_store::SqliteStore;
use uuid::Uuid;

use crate::common::{LOCAL_UID, OrPanic as _, TestDaemon, local_context, spawn_request};

type PortFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + Send + 'a>>;

const TEST_RUNTIME_PID: u32 = 42_424;

#[tokio::test]
pub(crate) async fn tx_b_failure_aborts_started_runtime_and_spawn_intent() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let runtime = Arc::new(FaultingRuntimePort::new(
        daemon.state.store.clone(),
        SpawnFault::FailTxBResolve,
    ));
    let state = daemon
        .state_with_runtime_port(Arc::clone(&runtime) as Arc<dyn RuntimePort>)
        .await;

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
        spawn_intent_status(&state.store, session_id)
            .await
            .as_deref(),
        Some("aborted")
    );
    assert_no_lifecycle(&state.store, session_id).await;
    assert_eq!(session_row_count(&state.store, session_id).await, 0);
}

#[tokio::test]
pub(crate) async fn abort_spawn_intent_clears_forking_and_marks_intent_aborted() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let runtime = Arc::new(FaultingRuntimePort::new(
        daemon.state.store.clone(),
        SpawnFault::FailRuntimeSpawn,
    ));
    let state = daemon
        .state_with_runtime_port(Arc::clone(&runtime) as Arc<dyn RuntimePort>)
        .await;

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
        spawn_intent_status(&state.store, session_id)
            .await
            .as_deref(),
        Some("aborted")
    );
    assert_no_lifecycle(&state.store, session_id).await;
    assert_eq!(session_row_count(&state.store, session_id).await, 0);
}

#[derive(Clone, Copy)]
enum SpawnFault {
    FailTxBResolve,
    FailRuntimeSpawn,
}

struct FaultingRuntimePort {
    store: SqliteStore,
    fault: SpawnFault,
    spawned_session_id: Mutex<Option<Uuid>>,
    terminated_session_ids: Mutex<Vec<Uuid>>,
}

impl FaultingRuntimePort {
    fn new(store: SqliteStore, fault: SpawnFault) -> Self {
        Self {
            store,
            fault,
            spawned_session_id: Mutex::new(None),
            terminated_session_ids: Mutex::new(Vec::new()),
        }
    }

    fn spawned_session_id(&self) -> Uuid {
        self.spawned_session_id
            .lock()
            .or_panic("spawned id lock succeeds")
            .or_panic("runtime spawn was attempted")
    }

    fn terminated(&self, session_id: Uuid) -> bool {
        self.terminated_session_ids
            .lock()
            .or_panic("terminated ids lock succeeds")
            .contains(&session_id)
    }

    async fn spawn_with_fault(
        &self,
        session_id: &str,
        launch: &SpawnLaunch,
    ) -> Result<SpawnedProcess, RuntimeError> {
        let session_id = parse_session_id(session_id)?;
        *self
            .spawned_session_id
            .lock()
            .or_panic("spawned id lock succeeds") = Some(session_id);
        match self.fault {
            SpawnFault::FailTxBResolve => {
                install_tx_b_resolve_failure(&self.store, session_id).await?;
                Ok(spawned_process(
                    session_id,
                    launch.runtime,
                    launch.isolation.clone(),
                ))
            }
            SpawnFault::FailRuntimeSpawn => {
                Err(RuntimeError::local("forced runtime spawn failure"))
            }
        }
    }
}

impl RuntimePort for FaultingRuntimePort {
    fn spawn<'a>(
        &'a self,
        session_id: &'a str,
        launch: &'a SpawnLaunch,
    ) -> PortFuture<'a, SpawnedProcess> {
        Box::pin(async move { self.spawn_with_fault(session_id, launch).await })
    }

    fn reap_exited(&self) -> PortFuture<'_, Vec<ChildExit>> {
        unsupported("reap_exited")
    }

    fn capture<'a>(
        &'a self,
        _session_id: &'a str,
        _scrollback_lines: Option<u32>,
    ) -> PortFuture<'a, CaptureResult> {
        unsupported("capture")
    }

    fn terminate<'a>(
        &'a self,
        session_id: &'a str,
        _signal: &'a str,
        _grace: Duration,
    ) -> PortFuture<'a, Option<ChildExit>> {
        Box::pin(async move {
            let session_id_uuid = parse_session_id(session_id)?;
            self.terminated_session_ids
                .lock()
                .or_panic("terminated ids lock succeeds")
                .push(session_id_uuid);
            Ok(Some(ChildExit {
                session_id: session_id.to_string(),
                runtime_pid: TEST_RUNTIME_PID,
                exit_code: Some(143),
                transcript_path: None,
            }))
        })
    }

    fn nudge<'a>(&'a self, _session_id: &'a str, _content: &'a str) -> PortFuture<'a, NudgeResult> {
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

fn parse_session_id(session_id: &str) -> Result<Uuid, RuntimeError> {
    Uuid::parse_str(session_id)
        .map_err(|_| RuntimeError::Fault(RuntimeFault::InvalidSessionId(session_id.to_string())))
}

fn spawned_process(
    session_id: Uuid,
    runtime: RuntimeKind,
    isolation: IsolationPolicy,
) -> SpawnedProcess {
    let mut lifecycle = Lifecycle::forking(session_id, runtime_kind(runtime));
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

fn runtime_kind(runtime: RuntimeKind) -> RuntimeRuntimeKind {
    match runtime {
        RuntimeKind::Claude => RuntimeRuntimeKind::Claude,
        RuntimeKind::Codex => RuntimeRuntimeKind::Codex,
    }
}

async fn install_tx_b_resolve_failure(
    store: &SqliteStore,
    session_id: Uuid,
) -> Result<(), RuntimeError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ws5_forced_resolve_failures (
            session_id TEXT PRIMARY KEY NOT NULL
        )",
    )
    .execute(store.pool())
    .await
    .map_err(|error| RuntimeError::local(format!("failed to create Tx-B fault table: {error}")))?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS ws5_fail_spawn_intent_resolve
         BEFORE UPDATE OF status ON session_spawn_intents
         FOR EACH ROW
         BEGIN
             SELECT RAISE(ABORT, 'forced Tx-B resolve failure')
             WHERE NEW.status = 'resolved'
               AND EXISTS (
                   SELECT 1
                   FROM ws5_forced_resolve_failures
                   WHERE session_id = NEW.session_id
               );
         END",
    )
    .execute(store.pool())
    .await
    .map_err(|error| {
        RuntimeError::local(format!("failed to create Tx-B fault trigger: {error}"))
    })?;
    sqlx::query("INSERT INTO ws5_forced_resolve_failures (session_id) VALUES (?)")
        .bind(session_id.to_string())
        .execute(store.pool())
        .await
        .map_err(|error| RuntimeError::local(format!("failed to install Tx-B fault: {error}")))?;
    Ok(())
}

async fn spawn_intent_status(store: &SqliteStore, session_id: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT status FROM session_spawn_intents WHERE session_id = ?")
        .bind(session_id.to_string())
        .fetch_optional(store.pool())
        .await
        .or_panic("spawn intent status query succeeds")
}

async fn session_row_count(store: &SqliteStore, session_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM session_sessions WHERE id = ?")
        .bind(session_id.to_string())
        .fetch_one(store.pool())
        .await
        .or_panic("session row count query succeeds")
}

async fn assert_no_lifecycle(store: &SqliteStore, session_id: Uuid) {
    let lifecycle = LifecycleStore::from_pool(store.pool().clone())
        .get(session_id)
        .await
        .or_panic("lifecycle query succeeds");
    assert!(
        lifecycle.is_none(),
        "expected no lifecycle for {session_id}"
    );
}
