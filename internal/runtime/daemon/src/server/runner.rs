use std::sync::Arc;

use anyhow::{Context, Result};
use lilo_db::LiloDb;
use lilo_identity_service::IdentityClient;
use lilo_paths::RuntimePathEnv;
use lilo_runtime_store::LifecycleStore;
use tokio::{net::UnixListener, sync::broadcast};

use crate::{handler, reconcile, socket};

use super::{DaemonConfig, ServerState};

pub async fn run_daemon(config: DaemonConfig) -> Result<()> {
    let db = LiloDb::open_path(&config.store.db_path).await?;
    run_daemon_with_db(config, db).await
}

pub async fn run_daemon_with_db(config: DaemonConfig, db: LiloDb) -> Result<()> {
    lilo_runtime_launchers::warm_registry().context("failed to initialize launcher registry")?;
    let store = LifecycleStore::open(&db);
    let identity = IdentityClient::from_db(&db, nix::unistd::getuid().as_raw());
    let socket_path = config.socket_path()?;
    socket::prepare_socket(socket_path)?;
    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    println!(
        "rtmd listening on {}",
        config
            .endpoint
            .display_label(&RuntimePathEnv::from_process())
    );

    let state = Arc::new(ServerState::new_with_identity(
        config.clone(),
        store,
        identity,
    )?);
    reconcile::reconcile_startup(Arc::clone(&state), &reconcile::SystemProcessProbe).await?;
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel(8);
    let reconcile_task = tokio::spawn(reconcile::run_periodic(
        Arc::clone(&state),
        reconcile::SystemProcessProbe,
        shutdown_tx.subscribe(),
        config.reconcile,
    ));
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("failed to accept daemon connection")?;
                let task_state = Arc::clone(&state);
                let task_shutdown = shutdown_tx.clone();
                tokio::spawn(async move {
                    if let Err(error) = handler::handle_connection(stream, task_state, task_shutdown).await {
                        tracing::warn!(%error, "daemon connection failed");
                    }
                });
            }
            _ = shutdown_rx.recv() => break,
            _ = tokio::signal::ctrl_c() => break,
            _ = terminate.recv() => break,
        }
    }

    socket::remove_socket_file(config.socket_path()?)?;
    let _ = shutdown_tx.send(());
    if let Err(error) = reconcile_task.await {
        tracing::warn!(%error, "periodic reconciliation task failed");
    }
    Ok(())
}
