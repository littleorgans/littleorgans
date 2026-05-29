use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use lilo_db::LiloDb;
use lilo_identity_service::IdentityClient;
use lilo_im_core::{Action, AuditDecision, AuditRow, Principal};
use lilo_im_store::{AuditFilters, SqliteAuditSink, query_audit};
use lilo_paths::{LiloHome, LiloPaths};
use lilo_rm_core::{
    HeadlessSpawnTarget, IsolationPolicy, KillRequest, RuntimeExit, RuntimeKind, RuntimeResponse,
    RuntimeRpc, RuntimeSignal, ShimExit, ShimLaunchRequest, ShimReady, SpawnRequest, SpawnTarget,
};
use lilo_runtime_store::{LifecycleStore, StoreConfig};
use uuid::Uuid;

use crate::server::ServerState;
use crate::{DaemonConfig, ReconcileConfig, docker_preflight::DockerPreflightConfig};

const LOCAL_UID: u32 = 42;

struct TestRuntime {
    state: Arc<ServerState>,
    db: LiloDb,
    paths: LiloPaths,
    _temp: tempfile::TempDir,
}

impl TestRuntime {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = LiloPaths::new(LiloHome::from_path(temp.path().join("lilo")).expect("home"));
        let db = LiloDb::open(&paths).await.expect("db");
        let store = LifecycleStore::open(&db);
        let identity = IdentityClient::new(
            SqliteAuditSink::with_pool(db.identity_pool().clone()),
            LOCAL_UID,
        );
        let state = Arc::new(
            ServerState::new_with_identity(config(&paths, temp.path()), store, identity)
                .expect("state"),
        );
        Self {
            state,
            db,
            paths,
            _temp: temp,
        }
    }

    async fn handle(&self, principal: Principal, rpc: RuntimeRpc) -> RuntimeResponse {
        super::handle_rpc(principal, rpc, Arc::clone(&self.state)).await
    }

    async fn audit_rows(&self) -> Vec<AuditRow> {
        query_audit(self.db.identity_pool(), AuditFilters::default())
            .await
            .expect("audit rows")
    }
}

#[tokio::test]
async fn non_local_spawn_and_kill_are_denied_before_runtime_work() {
    let runtime = TestRuntime::new().await;
    let principal = Principal::Local(LOCAL_UID + 1);
    let spawn_id = Uuid::now_v7();
    let kill_id = Uuid::now_v7();

    let spawn = runtime
        .handle(
            principal.clone(),
            RuntimeRpc::Spawn {
                request: spawn_request(spawn_id, &runtime.paths.run_root()),
            },
        )
        .await;
    let kill = runtime
        .handle(
            principal,
            RuntimeRpc::Kill {
                request: kill_request(kill_id),
            },
        )
        .await;

    assert_auth_error(&spawn);
    assert_auth_error(&kill);
    let rows = runtime.audit_rows().await;
    assert_decision(&rows[0], Action::Spawn, spawn_id, false);
    assert_decision(&rows[1], Action::Kill, kill_id, false);
}

#[tokio::test]
async fn local_spawn_and_kill_are_audited_before_runtime_errors() {
    let runtime = TestRuntime::new().await;
    let principal = Principal::Local(LOCAL_UID);
    let spawn_id = Uuid::now_v7();
    let kill_id = Uuid::now_v7();

    let spawn = runtime
        .handle(
            principal.clone(),
            RuntimeRpc::Spawn {
                request: spawn_request(spawn_id, &runtime.paths.run_root()),
            },
        )
        .await;
    let kill = runtime
        .handle(
            principal,
            RuntimeRpc::Kill {
                request: kill_request(kill_id),
            },
        )
        .await;

    assert_non_auth_error(&spawn);
    assert_non_auth_error(&kill);
    let rows = runtime.audit_rows().await;
    assert_decision(&rows[0], Action::Spawn, spawn_id, true);
    assert_decision(&rows[1], Action::Kill, kill_id, true);
    assert_session_tables_empty(&runtime.db, spawn_id).await;
}

#[tokio::test]
async fn shim_callbacks_use_named_local_only_policy() {
    let runtime = TestRuntime::new().await;

    for rpc in shim_rpcs() {
        let response = runtime.handle(Principal::Local(LOCAL_UID + 1), rpc).await;
        assert_auth_error(&response);
    }
    for rpc in shim_rpcs() {
        let response = runtime.handle(Principal::Local(LOCAL_UID), rpc).await;
        assert_non_auth_error(&response);
    }

    let rows = runtime.audit_rows().await;
    assert_eq!(rows.len(), 6);
    assert_eq!(
        rows.iter()
            .filter(|row| row.action == Action::ShimCallback)
            .count(),
        6
    );
    assert_eq!(
        rows.iter()
            .filter(|row| matches!(row.decision, AuditDecision::Deny { .. }))
            .count(),
        3
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.decision == AuditDecision::Allow)
            .count(),
        3
    );
}

fn config(paths: &LiloPaths, temp: &Path) -> DaemonConfig {
    DaemonConfig {
        endpoint: lilo_paths::RuntimeEndpoint::unix_socket(paths.socket_path()),
        shim_path: temp.join("missing-shim"),
        log_root: paths.logs_root(),
        store: StoreConfig {
            db_path: paths.db_path(),
        },
        reconcile: ReconcileConfig::default(),
        docker_preflight: DockerPreflightConfig::default(),
        tmux_server_label: None,
    }
}

fn spawn_request(session_id: Uuid, cwd: &Path) -> SpawnRequest {
    SpawnRequest {
        session_id,
        runtime: RuntimeKind::Codex,
        isolation: IsolationPolicy::default(),
        image: None,
        env: Vec::new(),
        mounts: Vec::new(),
        cwd: cwd.to_path_buf(),
        target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
        force: false,
        shell_resume: None,
    }
}

fn kill_request(session_id: Uuid) -> KillRequest {
    KillRequest {
        session_id,
        signal: RuntimeSignal::Term,
        grace_secs: 0,
    }
}

fn shim_rpcs() -> Vec<RuntimeRpc> {
    vec![
        RuntimeRpc::ShimLaunch {
            request: ShimLaunchRequest {
                session_id: Uuid::now_v7(),
            },
        },
        RuntimeRpc::ShimReady {
            ready: ShimReady {
                session_id: Uuid::now_v7(),
                shim_pid: 10,
                runtime_pid: 11,
                start_time: Utc::now(),
                tmux_pane: None,
            },
        },
        RuntimeRpc::ShimExit {
            exit: ShimExit {
                session_id: Uuid::now_v7(),
                exit: RuntimeExit::new(Some(1), None),
            },
        },
    ]
}

fn assert_auth_error(response: &RuntimeResponse) {
    let RuntimeResponse::Error(error) = response else {
        panic!("expected auth error, got {response:?}");
    };
    assert!(
        error.message.contains("authorization failed"),
        "{}",
        error.message
    );
}

fn assert_non_auth_error(response: &RuntimeResponse) {
    let RuntimeResponse::Error(error) = response else {
        return;
    };
    assert!(
        !error.message.contains("authorization failed"),
        "{}",
        error.message
    );
}

fn assert_decision(row: &AuditRow, action: Action, session_id: Uuid, should_allow: bool) {
    assert_eq!(row.action, action);
    assert_eq!(row.resource.session_id, Some(session_id));
    if should_allow {
        assert_eq!(row.decision, AuditDecision::Allow);
    } else {
        assert!(matches!(row.decision, AuditDecision::Deny { .. }));
    }
}

async fn assert_session_tables_empty(db: &LiloDb, session_id: Uuid) {
    let id = session_id.to_string();
    assert_eq!(
        count_rows(
            db.session_pool(),
            "SELECT COUNT(*) FROM session_spawn_intents WHERE session_id = ?",
            &id,
        )
        .await,
        0
    );
    assert_eq!(
        count_rows(
            db.session_pool(),
            "SELECT COUNT(*) FROM session_sessions WHERE id = ?",
            &id,
        )
        .await,
        0
    );
}

async fn count_rows(pool: &sqlx::SqlitePool, sql: &str, id: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count rows")
}
