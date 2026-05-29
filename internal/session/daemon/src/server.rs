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
use tokio::net::{UnixListener, UnixStream};

use crate::handler::DaemonState;
use crate::identity_client::{IdentityClient, RequestContext};
use crate::lifecycle::LifecycleTask;

pub async fn run_daemon(paths: LiloPaths) -> Result<()> {
    let db = LiloDb::open(&paths).await?;
    run_daemon_with_db(paths, db).await
}

pub async fn run_daemon_with_db(paths: LiloPaths, db: LiloDb) -> Result<()> {
    fs::create_dir_all(paths.run_root()).context("failed to create run directory")?;
    let endpoint = DaemonEndpoint::from_paths(&paths);
    remove_stale_socket(&endpoint)?;

    let listener =
        UnixListener::bind(endpoint.as_path()).context("failed to bind daemon socket")?;
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
        nix::unistd::getuid().as_raw(),
    );
    let state = Arc::new(DaemonState::new(
        store,
        Arc::new(runtime_port),
        Arc::new(identity),
        runtime,
    ));
    crate::reconcile::reconcile_once(&state)
        .await
        .context("failed to reconcile sessions on startup")?;
    let lifecycle = LifecycleTask::spawn(Arc::clone(&state));
    let events = crate::events::RuntimeEventTask::spawn(Arc::clone(&state));

    let result = serve(listener, &state).await;
    drop(events);
    drop(lifecycle);
    state.runtime.terminate_all();
    cleanup_paths(&paths, &endpoint);
    result
}

async fn serve(listener: UnixListener, state: &DaemonState) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await.context("failed to accept client")?;
        if handle_connection(stream, state).await? {
            return Ok(());
        }
    }
}

async fn handle_connection(mut stream: UnixStream, state: &DaemonState) -> Result<bool> {
    let principal = match lilo_im_core::peer_creds::extract(&stream).await {
        Ok(principal) => principal,
        Err(error) => {
            return write_response(
                stream,
                crate::handler::HandlerResult {
                    response: RpcResponse::Error {
                        message: error.to_string(),
                    },
                    shutdown: false,
                },
            )
            .await;
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

    write_response(stream, result).await
}

async fn write_response(
    mut stream: UnixStream,
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

fn remove_stale_socket(endpoint: &DaemonEndpoint) -> Result<()> {
    if endpoint.as_path().exists() {
        fs::remove_file(endpoint.as_path()).context("failed to remove stale socket")?;
    }
    Ok(())
}

fn cleanup_paths(paths: &LiloPaths, endpoint: &DaemonEndpoint) {
    let _ = fs::remove_file(endpoint.as_path());
    let _ = fs::remove_file(paths.pid_path());
}
