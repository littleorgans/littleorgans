use std::sync::Arc;

use anyhow::{Context, Result};
use lilo_db::LiloDb;
use tokio::task::JoinSet;

use crate::handler;

use super::{DaemonConfig, prepare_runtime_bootstrap, start_runtime_reconcile};

pub async fn run_daemon(config: DaemonConfig) -> Result<()> {
    let db = LiloDb::open_postgres_resolved().await?;
    run_daemon_with_db(config, db).await
}

pub async fn run_daemon_with_db(config: DaemonConfig, db: LiloDb) -> Result<()> {
    let bootstrap = prepare_runtime_bootstrap(&config, &db, lilo_sys::creds::current_uid())?;
    let socket_path = &bootstrap.socket_path;
    let listener = lilo_sys::ipc::bind(socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    println!("lilod listening on {}", config.endpoint.display_label());

    let state = bootstrap.into_state(config.clone())?;
    let reconcile = start_runtime_reconcile(Arc::clone(&state), config.reconcile).await?;
    let shutdown_tx = reconcile.shutdown_tx;
    let mut shutdown_rx = shutdown_tx.subscribe();
    let shutdown_signal = lilo_sys::signal::on_shutdown()?;
    tokio::pin!(shutdown_signal);
    let mut connections: JoinSet<()> = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let stream = accepted.context("failed to accept daemon connection")?;
                let task_state = Arc::clone(&state);
                let task_shutdown = shutdown_tx.clone();
                connections.spawn(async move {
                    if let Err(error) = handler::handle_connection(stream, task_state, task_shutdown).await {
                        tracing::warn!(%error, "daemon connection failed");
                    }
                });
            }
            _ = shutdown_rx.recv() => break,
            () = &mut shutdown_signal => break,
        }
    }

    lilo_sys::ipc::remove_socket_file(config.socket_path()?)?;
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
