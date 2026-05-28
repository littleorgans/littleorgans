use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use lilo_db::LiloDb;
use lilo_identity_service::IdentityClient;
use lilo_runtime_store::LifecycleStore;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::reconcile::{self, ReconcileConfig};

use super::{DaemonConfig, ServerState};

pub(crate) struct RuntimeBootstrap {
    pub(crate) socket_path: PathBuf,
    store: LifecycleStore,
    identity: IdentityClient,
}

impl RuntimeBootstrap {
    pub(crate) fn into_state(self, config: DaemonConfig) -> Result<Arc<ServerState>> {
        Ok(Arc::new(ServerState::new_with_identity(
            config,
            self.store,
            self.identity,
        )?))
    }
}

pub(crate) struct RuntimeReconcile {
    pub(crate) shutdown_tx: broadcast::Sender<()>,
    pub(crate) reconcile_task: JoinHandle<()>,
}

pub(crate) fn prepare_runtime_bootstrap(
    config: &DaemonConfig,
    db: &LiloDb,
    local_uid: u32,
) -> Result<RuntimeBootstrap> {
    lilo_runtime_launchers::warm_registry().context("failed to initialize launcher registry")?;
    Ok(RuntimeBootstrap {
        socket_path: config.socket_path()?.to_path_buf(),
        store: LifecycleStore::open(db),
        identity: IdentityClient::from_db(db, local_uid),
    })
}

pub(crate) async fn start_runtime_reconcile(
    state: Arc<ServerState>,
    config: ReconcileConfig,
) -> Result<RuntimeReconcile> {
    reconcile::reconcile_startup(Arc::clone(&state), &reconcile::SystemProcessProbe).await?;
    let (shutdown_tx, _) = broadcast::channel(8);
    let reconcile_task = tokio::spawn(reconcile::run_periodic(
        state,
        reconcile::SystemProcessProbe,
        shutdown_tx.subscribe(),
        config,
    ));
    Ok(RuntimeReconcile {
        shutdown_tx,
        reconcile_task,
    })
}
