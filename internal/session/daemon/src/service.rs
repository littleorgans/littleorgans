use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result};
use lilo_db::LiloDb;
use lilo_im_core::Principal;
use lilo_im_store::SqliteAuditSink;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_runtime_daemon::{DaemonConfig, RuntimeService, RuntimeServiceContext};
use lilo_session_core::{RpcResponse, SessionRpc};
use lilo_session_driver::InProcessRuntime;
use lilo_session_store::SqliteStore;

use crate::handler::{DaemonState, HandlerResult};
use crate::identity_client::{IdentityClient, RequestContext};
use crate::{events::RuntimeEventTask, lifecycle::LifecycleTask};

pub struct SessionServiceContext {
    paths: LiloPaths,
    db: LiloDb,
    runtime: Arc<RuntimeService>,
}

impl SessionServiceContext {
    pub fn new(paths: LiloPaths, db: LiloDb, runtime: Arc<RuntimeService>) -> Self {
        Self { paths, db, runtime }
    }

    pub async fn from_env() -> Result<Self> {
        let home = LiloHome::from_env().context("failed to resolve lilo home")?;
        let paths = LiloPaths::new(home);
        let db = LiloDb::open(&paths).await?;
        let runtime_config = DaemonConfig::from_lilo_paths(&paths)?;
        let runtime = Arc::new(
            RuntimeService::build(RuntimeServiceContext::new(runtime_config, db.clone())).await?,
        );
        Ok(Self::new(paths, db, runtime))
    }

    pub fn paths(&self) -> &LiloPaths {
        &self.paths
    }

    pub fn into_parts(self) -> (LiloPaths, LiloDb, Arc<RuntimeService>) {
        (self.paths, self.db, self.runtime)
    }
}

pub struct SessionService {
    paths: LiloPaths,
    state: Arc<DaemonState>,
    lifecycle: LifecycleTask,
    events: RuntimeEventTask,
}

impl SessionService {
    pub fn build(ctx: SessionServiceContext) -> Result<Self> {
        let (paths, db, runtime) = ctx.into_parts();
        fs::create_dir_all(paths.run_root()).context("failed to create run directory")?;
        let store = SqliteStore::open(&db);
        let runtime_port = InProcessRuntime::new(Arc::clone(&runtime));
        let identity = IdentityClient::new(
            SqliteAuditSink::with_pool(db.identity_pool().clone()),
            nix::unistd::getuid().as_raw(),
        );
        let state = Arc::new(
            DaemonState::new(store, Arc::new(runtime_port), Arc::new(identity), runtime)
                .with_rtmd_socket_path(paths.socket_path()),
        );
        let lifecycle = LifecycleTask::spawn(Arc::clone(&state));
        let events = RuntimeEventTask::spawn(Arc::clone(&state));
        Ok(Self {
            paths,
            state,
            lifecycle,
            events,
        })
    }

    pub fn paths(&self) -> &LiloPaths {
        &self.paths
    }

    pub async fn handle_rpc(&self, principal: Principal, request: SessionRpc) -> HandlerResult {
        self.state
            .handle(RequestContext::new(principal), request)
            .await
    }

    pub async fn reconcile_pending_spawn_intents(&self) -> Result<()> {
        self.state.reconcile_pending_spawn_intents().await
    }

    pub fn shutdown_response(message: impl Into<String>) -> HandlerResult {
        HandlerResult {
            response: RpcResponse::Error {
                message: message.into(),
            },
            shutdown: false,
        }
    }
}

impl Drop for SessionService {
    fn drop(&mut self) {
        let _ = &self.lifecycle;
        let _ = &self.events;
        self.state.runtime.terminate_all();
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionService, SessionServiceContext};
    use chrono::Utc;
    use lilo_db::LiloDb;
    use lilo_paths::{LiloHome, LiloPaths};
    use lilo_rm_core::{
        HeadlessSpawnTarget, IsolationPolicy, Lifecycle, RuntimeKind as RuntimeRuntimeKind,
        ShimReady, SpawnRequest as RuntimeSpawnRequest, SpawnTarget,
    };
    use lilo_runtime_daemon::{DaemonConfig, RuntimeService, RuntimeServiceContext};
    use lilo_runtime_store::LifecycleStore;
    use lilo_session_core::{Namespace, RuntimeKind, Session, SessionState};
    use lilo_session_store::{PendingSpawnIntent, SessionDraft, SqliteStore};
    use std::sync::Arc;
    use uuid::Uuid;

    #[tokio::test]
    async fn build_preserves_session_paths_for_later_composition() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = LiloPaths::new(LiloHome::from_path(dir.path().join("lilo")).expect("home"));
        let db = LiloDb::open(&paths).await.expect("db");
        let runtime = Arc::new(
            RuntimeService::build(RuntimeServiceContext::new(
                DaemonConfig::from_lilo_paths(&paths).expect("runtime config"),
                db.clone(),
            ))
            .await
            .expect("runtime service"),
        );

        let service = SessionService::build(SessionServiceContext::new(paths.clone(), db, runtime))
            .expect("service builds");

        assert_eq!(service.paths().socket_path(), paths.socket_path());
    }

    #[tokio::test]
    async fn reconcile_pending_spawn_intent_completes_running_lifecycle() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = LiloPaths::new(LiloHome::from_path(dir.path().join("lilo")).expect("home"));
        let db = LiloDb::open(&paths).await.expect("db");
        let runtime = Arc::new(
            RuntimeService::build(RuntimeServiceContext::new(
                DaemonConfig::from_lilo_paths(&paths).expect("runtime config"),
                db.clone(),
            ))
            .await
            .expect("runtime service"),
        );
        let session_store = SqliteStore::open(&db);
        let lifecycle_store = LifecycleStore::open(&db);
        let session_id = Uuid::now_v7();
        let draft = SessionDraft::new(&draft_session(session_id));
        session_store
            .insert_pending_spawn_intent(&PendingSpawnIntent::new(
                Uuid::now_v7(),
                runtime_request(session_id),
                draft,
            ))
            .await
            .expect("insert pending intent");
        let running = running_lifecycle(session_id);
        let mut forking = Lifecycle::forking(session_id, RuntimeRuntimeKind::Claude);
        forking.isolation = IsolationPolicy::Host;
        lifecycle_store
            .insert_forking(&forking)
            .await
            .expect("insert forking lifecycle");
        lifecycle_store
            .update_lifecycle(&running)
            .await
            .expect("update running lifecycle");

        let service = SessionService::build(SessionServiceContext::new(
            paths,
            db.clone(),
            Arc::clone(&runtime),
        ))
        .expect("service builds");
        service
            .reconcile_pending_spawn_intents()
            .await
            .expect("reconcile pending intents");

        assert!(
            session_store
                .get_session(&session_id)
                .await
                .expect("get session")
                .is_some()
        );
        assert!(
            session_store
                .list_pending_spawn_intents()
                .await
                .expect("list pending")
                .is_empty()
        );
    }

    fn draft_session(id: Uuid) -> Session {
        let now = Utc::now();
        Session {
            id,
            runtime: RuntimeKind::Claude,
            role: "worker".to_owned(),
            workspace: "/tmp".to_owned(),
            namespace: Namespace::default(),
            dir: std::path::PathBuf::from("/tmp"),
            labels: Vec::new(),
            state: SessionState::Running,
            runtime_pid: 0,
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

    fn runtime_request(session_id: Uuid) -> RuntimeSpawnRequest {
        RuntimeSpawnRequest {
            session_id,
            runtime: RuntimeRuntimeKind::Claude,
            isolation: IsolationPolicy::Host,
            image: None,
            env: Vec::new(),
            mounts: Vec::new(),
            cwd: std::path::PathBuf::from("/tmp"),
            target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
            force: false,
            shell_resume: None,
        }
    }

    fn running_lifecycle(session_id: Uuid) -> Lifecycle {
        let mut lifecycle = Lifecycle::forking(session_id, RuntimeRuntimeKind::Claude);
        assert!(lifecycle.mark_running(ShimReady {
            session_id,
            shim_pid: 1,
            runtime_pid: 2,
            start_time: Utc::now(),
            tmux_pane: None,
        }));
        lifecycle
    }
}
