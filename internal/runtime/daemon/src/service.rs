use std::sync::Arc;

use crate::server::{DaemonConfig, ServerState};
use crate::{handler, reconcile};
use anyhow::{Context, Result};
use lilo_db::LiloDb;
use lilo_identity_service::IdentityClient;
use lilo_im_core::Principal;
use lilo_rm_core::{RuntimeEvent, RuntimeResponse, RuntimeRpc};
use lilo_runtime_store::LifecycleStore;
use tokio::sync::{Mutex, broadcast};
use tokio::task::JoinHandle;

pub struct RuntimeServiceContext {
    config: DaemonConfig,
    db: LiloDb,
    local_uid: u32,
}

impl RuntimeServiceContext {
    pub fn new(config: DaemonConfig, db: LiloDb) -> Self {
        Self::new_with_local_uid(config, db, nix::unistd::getuid().as_raw())
    }

    pub fn new_with_local_uid(config: DaemonConfig, db: LiloDb, local_uid: u32) -> Self {
        Self {
            config,
            db,
            local_uid,
        }
    }

    pub async fn from_env() -> Result<Self> {
        let config = DaemonConfig::from_env()?;
        let db = LiloDb::open_path(&config.store.db_path).await?;
        Ok(Self::new(config, db))
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub fn into_parts(self) -> (DaemonConfig, LiloDb, u32) {
        (self.config, self.db, self.local_uid)
    }
}

pub struct RuntimeService {
    config: DaemonConfig,
    state: Arc<ServerState>,
    shutdown_tx: broadcast::Sender<()>,
    reconcile_task: Mutex<Option<JoinHandle<()>>>,
}

impl RuntimeService {
    pub async fn build(ctx: RuntimeServiceContext) -> Result<Self> {
        let (config, db, local_uid) = ctx.into_parts();
        lilo_runtime_launchers::warm_registry()
            .context("failed to initialize launcher registry")?;
        let store = LifecycleStore::open(&db);
        let identity = IdentityClient::from_db(&db, local_uid);
        let _ = config.socket_path()?;
        let state = Arc::new(ServerState::new_with_identity(
            config.clone(),
            store,
            identity,
        )?);
        reconcile::reconcile_startup(Arc::clone(&state), &reconcile::SystemProcessProbe).await?;
        let (shutdown_tx, _) = broadcast::channel(8);
        let reconcile_task = tokio::spawn(reconcile::run_periodic(
            Arc::clone(&state),
            reconcile::SystemProcessProbe,
            shutdown_tx.subscribe(),
            config.reconcile,
        ));
        Ok(Self {
            config,
            state,
            shutdown_tx,
            reconcile_task: Mutex::new(Some(reconcile_task)),
        })
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub async fn handle_rpc(&self, principal: Principal, rpc: RuntimeRpc) -> RuntimeResponse {
        let response = handler::handle_rpc(principal, rpc, Arc::clone(&self.state)).await;
        if matches!(response, RuntimeResponse::Stopping) {
            let _ = self.shutdown_tx.send(());
        }
        response
    }

    pub async fn append_event(&self, event: RuntimeEvent) -> Result<RuntimeEvent> {
        self.state.append_event(event).await
    }

    pub fn subscribe_shutdown(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    pub async fn shutdown(&self) -> Result<()> {
        let _ = self.shutdown_tx.send(());
        let reconcile_task = {
            let mut task = self.reconcile_task.lock().await;
            task.take()
        };
        if let Some(task) = reconcile_task {
            task.await.context("periodic reconciliation task failed")?;
        }
        Ok(())
    }
}

impl Drop for RuntimeService {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeService, RuntimeServiceContext};
    use crate::{DaemonConfig, ReconcileConfig, docker_preflight::DockerPreflightConfig};
    use lilo_db::LiloDb;
    use lilo_paths::{LiloHome, LiloPaths};
    use lilo_runtime_store::StoreConfig;
    use std::time::Duration;

    #[tokio::test]
    async fn build_preserves_daemon_config_for_later_composition() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = LiloPaths::new(LiloHome::from_path(dir.path().join("lilo")).expect("home"));
        let config = DaemonConfig {
            endpoint: lilo_paths::RuntimeEndpoint::unix_socket(paths.socket_path()),
            shim_path: dir.path().join("shim"),
            log_root: paths.logs_root(),
            store: StoreConfig {
                db_path: paths.db_path(),
            },
            reconcile: ReconcileConfig::default(),
            docker_preflight: DockerPreflightConfig::default(),
        };
        let db = LiloDb::open(&paths).await.expect("db");

        let service = RuntimeService::build(RuntimeServiceContext::new(config.clone(), db))
            .await
            .expect("service builds");

        assert_eq!(
            service.config().socket_path().expect("socket"),
            config.socket_path().expect("socket")
        );
    }

    #[tokio::test]
    async fn runtime_shutdown_drains_periodic_reconcile_task() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = LiloPaths::new(LiloHome::from_path(dir.path().join("lilo")).expect("home"));
        let config = DaemonConfig {
            endpoint: lilo_paths::RuntimeEndpoint::unix_socket(paths.socket_path()),
            shim_path: dir.path().join("shim"),
            log_root: paths.logs_root(),
            store: StoreConfig {
                db_path: paths.db_path(),
            },
            reconcile: ReconcileConfig {
                sweep_interval: Duration::from_mins(1),
                resume_poll_interval: Duration::from_mins(1),
                ..ReconcileConfig::default()
            },
            docker_preflight: DockerPreflightConfig::default(),
        };
        let db = LiloDb::open(&paths).await.expect("db");
        let service = RuntimeService::build(RuntimeServiceContext::new(config, db.clone()))
            .await
            .expect("service builds");

        tokio::time::timeout(Duration::from_millis(100), service.shutdown())
            .await
            .expect("shutdown returns before timeout")
            .expect("shutdown succeeds");
        service.shutdown().await.expect("second shutdown succeeds");
        db.close().await;
    }
}
