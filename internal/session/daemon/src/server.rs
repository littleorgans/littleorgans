use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result};
use lilo_db::LiloDb;
use lilo_im_store::SqliteAuditSink;
use lilo_paths::{DaemonEndpoint, LiloPaths};
use lilo_runtime_daemon::{DaemonConfig, RuntimeService, RuntimeServiceContext};
use lilo_session_core::{RpcResponse, SessionRpc};
use lilo_session_driver::InProcessRuntime;
use lilo_session_store::SqliteStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
use tokio::task::{JoinError, JoinSet};

use crate::handler::DaemonState;
use crate::identity_client::{IdentityClient, RequestContext};
use crate::lifecycle::LifecycleTask;

pub async fn run_daemon(paths: LiloPaths, daemon_version: impl Into<String>) -> Result<()> {
    let daemon_version = daemon_version.into();
    let db = LiloDb::open(&paths).await?;
    run_daemon_with_db(paths, daemon_version, db).await
}

pub async fn run_daemon_with_db(
    paths: LiloPaths,
    daemon_version: impl Into<String>,
    db: LiloDb,
) -> Result<()> {
    let daemon_version = daemon_version.into();
    fs::create_dir_all(paths.run_root()).context("failed to create run directory")?;
    let endpoint = DaemonEndpoint::from_paths(&paths);

    let listener =
        lilo_sys::ipc::bind(endpoint.as_path()).context("failed to bind daemon socket")?;
    fs::write(paths.pid_path(), std::process::id().to_string())
        .context("failed to write pidfile")?;

    let store = SqliteStore::open(&db);
    let runtime = Arc::new(
        RuntimeService::build(RuntimeServiceContext::new(
            DaemonConfig::from_lilo_paths(&paths)?,
            db.clone(),
        ))
        .await
        .context("failed to build runtime service")?,
    );
    let runtime_port = InProcessRuntime::new(Arc::clone(&runtime));
    let identity = IdentityClient::new(
        SqliteAuditSink::with_pool(db.identity_pool().clone()),
        lilo_sys::creds::current_uid(),
    );
    let state = Arc::new(DaemonState::new(
        store,
        daemon_version,
        Arc::new(runtime_port),
        Arc::new(identity),
        runtime,
    ));
    crate::reconcile::reconcile_once(&state)
        .await
        .context("failed to reconcile sessions on startup")?;
    let lifecycle = LifecycleTask::spawn(Arc::clone(&state));
    let events = crate::events::RuntimeEventTask::spawn(Arc::clone(&state));

    let result = serve(listener, Arc::clone(&state)).await;
    drop(events);
    drop(lifecycle);
    state.runtime.terminate_all();
    cleanup_paths(&paths, &endpoint);
    result
}

async fn serve(listener: lilo_sys::ipc::IpcListener, state: Arc<DaemonState>) -> Result<()> {
    let shutdown = Arc::new(Notify::new());
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            stream = listener.accept() => {
                let stream = stream.context("failed to accept client")?;
                let state = Arc::clone(&state);
                let shutdown = Arc::clone(&shutdown);
                connections.spawn(async move {
                    handle_connection(stream, state, shutdown).await
                });
            }
            () = shutdown.notified() => {
                break;
            }
            Some(result) = connections.join_next(), if !connections.is_empty() => {
                log_connection_result(result);
            }
        }
    }
    connections.abort_all();
    while let Some(result) = connections.join_next().await {
        log_connection_result(result);
    }
    Ok(())
}

async fn handle_connection(
    mut stream: lilo_sys::ipc::IpcStream,
    state: Arc<DaemonState>,
    shutdown: Arc<Notify>,
) -> Result<()> {
    let principal = match lilo_im_core::peer_creds::extract(&stream).await {
        Ok(principal) => principal,
        Err(error) => {
            write_response(
                stream,
                crate::handler::HandlerResult {
                    response: RpcResponse::Error {
                        message: error.to_string(),
                    },
                    shutdown: false,
                },
            )
            .await?;
            return Ok(());
        }
    };

    let mut request_bytes = Vec::new();
    stream
        .read_to_end(&mut request_bytes)
        .await
        .context("failed to read request")?;

    let result = match serde_json::from_slice::<SessionRpc>(&request_bytes) {
        Ok(request) => state.handle(RequestContext::new(principal), request).await,
        Err(error) => crate::handler::HandlerResult {
            response: RpcResponse::Error {
                message: error.to_string(),
            },
            shutdown: false,
        },
    };

    if write_response(stream, result).await? {
        shutdown.notify_one();
    }
    Ok(())
}

async fn write_response(
    mut stream: lilo_sys::ipc::IpcStream,
    result: crate::handler::HandlerResult,
) -> Result<bool> {
    let response = serde_json::to_vec(&result.response).context("failed to encode response")?;
    stream
        .write_all(&response)
        .await
        .context("failed to write response")?;
    stream
        .shutdown()
        .await
        .context("failed to close response")?;

    Ok(result.shutdown)
}

fn log_connection_result(result: Result<Result<()>, JoinError>) {
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(error = ?error, "session daemon connection failed");
        }
        Err(error) if error.is_cancelled() => {}
        Err(error) => {
            tracing::warn!(error = ?error, "session daemon connection task failed");
        }
    }
}

fn cleanup_paths(paths: &LiloPaths, endpoint: &DaemonEndpoint) {
    let _ = lilo_sys::ipc::remove_socket_file(endpoint.as_path());
    let _ = fs::remove_file(paths.pid_path());
}
