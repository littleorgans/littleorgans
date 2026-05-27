use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use lilo_db::LiloDb;
use lilo_im_store::SqliteAuditSink;
use lilo_rm_client::RuntimeClient;
use lilo_rm_core::RUNTIME_PROTOCOL_VERSION;
use lilo_runtime_daemon::{DaemonConfig, RuntimeService, RuntimeServiceContext};
use lilo_session_core::{RpcResponse, SessionRpc, SmEndpoint, SmPaths, rtmd_socket_path};
use lilo_session_driver::RtmdDriver;
use lilo_session_store::SqliteStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::handler::DaemonState;
use crate::identity_client::{IdentityClient, RequestContext};
use crate::lifecycle::LifecycleTask;

pub async fn run_daemon(paths: SmPaths) -> Result<()> {
    let db = LiloDb::open_path(&paths.database).await?;
    run_daemon_with_db(paths, db).await
}

pub async fn run_daemon_with_db(paths: SmPaths, db: LiloDb) -> Result<()> {
    fs::create_dir_all(&paths.dir).context("failed to create runtime directory")?;
    let endpoint = SmEndpoint::from_env().context("failed to resolve daemon endpoint")?;
    let rtmd_socket_path = rtmd_socket_path();
    probe_rtmd(&rtmd_socket_path).await?;
    remove_stale_socket(&endpoint)?;

    let listener =
        UnixListener::bind(endpoint.as_path()).context("failed to bind daemon socket")?;
    fs::write(&paths.pidfile, std::process::id().to_string()).context("failed to write pidfile")?;

    let store = SqliteStore::open(&db);
    let driver = RtmdDriver::new(rtmd_socket_path.clone());
    let runtime = Arc::new(
        RuntimeService::build(RuntimeServiceContext::new(
            DaemonConfig::from_env()?,
            db.clone(),
        ))
        .await
        .context("failed to build runtime service")?,
    );
    let identity = IdentityClient::new(
        SqliteAuditSink::with_pool(db.identity_pool().clone()),
        nix::unistd::getuid().as_raw(),
    );
    let state = Arc::new(
        DaemonState::new(store, Arc::new(driver), Arc::new(identity), runtime)
            .with_rtmd_socket_path(rtmd_socket_path.clone()),
    );
    crate::reconcile::reconcile_once(&state)
        .await
        .context("failed to reconcile sessions on startup")?;
    let lifecycle = LifecycleTask::spawn(Arc::clone(&state));
    let events = crate::events::RuntimeEventTask::spawn(Arc::clone(&state), rtmd_socket_path);

    let result = serve(listener, &state).await;
    drop(events);
    drop(lifecycle);
    state.driver.terminate_all();
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

fn remove_stale_socket(endpoint: &SmEndpoint) -> Result<()> {
    if endpoint.exists() {
        fs::remove_file(endpoint.as_path()).context("failed to remove stale socket")?;
    }
    Ok(())
}

fn cleanup_paths(paths: &SmPaths, endpoint: &SmEndpoint) {
    let _ = fs::remove_file(endpoint.as_path());
    let _ = fs::remove_file(&paths.pidfile);
}

async fn probe_rtmd(socket_path: &Path) -> Result<()> {
    let client = RuntimeClient::new(socket_path.to_path_buf());
    let payload = client
        .version()
        .await
        .with_context(|| format!("rtmd unavailable at {}:", socket_path.display()))?;
    let minimum = protocol_version_pair(RUNTIME_PROTOCOL_VERSION)
        .context("failed to parse required runtime protocol version")?;
    let actual = protocol_version_pair(&payload.version.protocol_version).with_context(|| {
        format!(
            "failed to parse rtmd protocol version {}",
            payload.version.protocol_version
        )
    })?;
    if actual < minimum {
        bail!(
            "rtmd protocol incompatible at {}: required >= {}, got {}",
            socket_path.display(),
            RUNTIME_PROTOCOL_VERSION,
            payload.version.protocol_version
        );
    }
    Ok(())
}

fn protocol_version_pair(version: &str) -> Result<(u64, u64)> {
    let mut parts = version.split('.');
    let major = parts
        .next()
        .context("protocol version missing major component")?
        .parse()
        .context("protocol version major component is not numeric")?;
    let minor = parts
        .next()
        .context("protocol version missing minor component")?
        .parse()
        .context("protocol version minor component is not numeric")?;
    Ok((major, minor))
}
