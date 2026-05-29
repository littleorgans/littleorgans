use std::sync::Arc;

use crate::handler;
use crate::server::{
    DaemonConfig, ServerState, prepare_runtime_bootstrap, start_runtime_reconcile,
};
use anyhow::{Context, Result};
use lilo_db::LiloDb;
use lilo_im_core::Principal;
use lilo_rm_core::{RuntimeEvent, RuntimeResponse, RuntimeRpc};
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
        let bootstrap = prepare_runtime_bootstrap(&config, &db, local_uid)?;
        let state = bootstrap.into_state(config.clone())?;
        let reconcile = start_runtime_reconcile(Arc::clone(&state), config.reconcile).await?;
        Ok(Self {
            config,
            state,
            shutdown_tx: reconcile.shutdown_tx,
            reconcile_task: Mutex::new(Some(reconcile.reconcile_task)),
        })
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub(crate) fn state(&self) -> &Arc<ServerState> {
        &self.state
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
        self.state.drain_shims();
        let reconcile_task = {
            let mut task = self.reconcile_task.lock().await;
            task.take()
        };
        if let Some(task) = reconcile_task {
            task.await.context("periodic reconciliation task failed")?;
        }
        Ok(())
    }

    /// Reap shims spawned by this service. Public so in-process owners (the
    /// session daemon, test harnesses) can drain without a full async shutdown.
    pub fn drain_shims(&self) {
        self.state.drain_shims();
    }
}

impl Drop for RuntimeService {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        // Catch-all so shims never outlive an owner that dropped the service
        // without an explicit shutdown (e.g. a test harness with no teardown).
        self.state.drain_shims();
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
        let fixture = ServiceFixture::new(ReconcileConfig::default()).await;

        let service = RuntimeService::build(fixture.context())
            .await
            .expect("service builds");

        assert_eq!(
            service.config().socket_path().expect("socket"),
            fixture.config.socket_path().expect("socket")
        );
    }

    #[tokio::test]
    async fn runtime_shutdown_drains_periodic_reconcile_task() {
        let fixture = ServiceFixture::new(ReconcileConfig {
            sweep_interval: Duration::from_mins(1),
            resume_poll_interval: Duration::from_mins(1),
            ..ReconcileConfig::default()
        })
        .await;
        let service = RuntimeService::build(fixture.context())
            .await
            .expect("service builds");

        tokio::time::timeout(Duration::from_millis(100), service.shutdown())
            .await
            .expect("shutdown returns before timeout")
            .expect("shutdown succeeds");
        service.shutdown().await.expect("second shutdown succeeds");
        fixture.db.close().await;
    }

    struct ServiceFixture {
        _dir: tempfile::TempDir,
        config: DaemonConfig,
        db: LiloDb,
    }

    impl ServiceFixture {
        async fn new(reconcile: ReconcileConfig) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let paths = LiloPaths::new(LiloHome::from_path(dir.path().join("lilo")).expect("home"));
            let config = DaemonConfig {
                endpoint: lilo_paths::RuntimeEndpoint::unix_socket(paths.socket_path()),
                shim_path: dir.path().join("shim"),
                log_root: paths.logs_root(),
                store: StoreConfig {
                    db_path: paths.db_path(),
                },
                reconcile,
                docker_preflight: DockerPreflightConfig::default(),
            };
            let db = LiloDb::open(&paths).await.expect("db");

            Self {
                _dir: dir,
                config,
                db,
            }
        }

        fn context(&self) -> RuntimeServiceContext {
            RuntimeServiceContext::new(self.config.clone(), self.db.clone())
        }
    }
}
