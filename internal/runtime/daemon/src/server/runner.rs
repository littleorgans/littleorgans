use std::sync::Arc;

use anyhow::{Context, Result};
use lilo_db::LiloDb;
use tokio::net::UnixListener;
use tokio::task::JoinSet;

use crate::{handler, socket};

use super::{DaemonConfig, prepare_runtime_bootstrap, start_runtime_reconcile};

pub async fn run_daemon(config: DaemonConfig) -> Result<()> {
    let db = LiloDb::open_path(&config.store.db_path).await?;
    run_daemon_with_db(config, db).await
}

pub async fn run_daemon_with_db(config: DaemonConfig, db: LiloDb) -> Result<()> {
    let bootstrap = prepare_runtime_bootstrap(&config, &db, nix::unistd::getuid().as_raw())?;
    let socket_path = &bootstrap.socket_path;
    socket::prepare_socket(socket_path)?;
    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    println!("lilod listening on {}", config.endpoint.display_label());

    let state = bootstrap.into_state(config.clone())?;
    let reconcile = start_runtime_reconcile(Arc::clone(&state), config.reconcile).await?;
    let shutdown_tx = reconcile.shutdown_tx;
    let mut shutdown_rx = shutdown_tx.subscribe();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut connections: JoinSet<()> = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("failed to accept daemon connection")?;
                let task_state = Arc::clone(&state);
                let task_shutdown = shutdown_tx.clone();
                connections.spawn(async move {
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
    // Drain in-flight connection handlers (they observe the shutdown
    // broadcast) so their sockets are released before we return, rather
    // than leaving detached tasks alive past daemon shutdown.
    while connections.join_next().await.is_some() {}
    // Tear down shims this daemon spawned so they do not outlive it as orphans.
    state.drain_shims();
    if let Err(error) = reconcile.reconcile_task.await {
        tracing::warn!(%error, "periodic reconciliation task failed");
    }
    Ok(())
}
