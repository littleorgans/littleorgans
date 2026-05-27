use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result};
use lilo_db::LiloDb;
use lilo_im_core::peer_creds;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_rm_core::{ErrorCode, RuntimeResponse, read_json_line, write_json_line};
use lilo_runtime_daemon::{DaemonConfig, RuntimeService, RuntimeServiceContext};
use lilo_session_core::RpcResponse;
use lilo_session_daemon::{SessionService, SessionServiceContext};
use lilo_wire::LilodRpc;
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub async fn run_from_env() -> Result<()> {
    let home = LiloHome::from_env().context("failed to resolve lilo home")?;
    run(LiloPaths::new(home)).await
}

pub async fn run(paths: LiloPaths) -> Result<()> {
    fs::create_dir_all(paths.run_root()).context("failed to create run directory")?;
    let db = LiloDb::open(&paths).await?;
    let runtime_config = DaemonConfig::from_lilo_paths(&paths)?;
    let runtime = Arc::new(
        RuntimeService::build(RuntimeServiceContext::new(runtime_config, db.clone())).await?,
    );
    let session = Arc::new(SessionService::build(SessionServiceContext::new(
        paths.clone(),
        db.clone(),
        Arc::clone(&runtime),
    ))?);
    session
        .reconcile_pending_spawn_intents()
        .await
        .context("failed to reconcile pending session spawn intents")?;

    let socket_path = paths.socket_path();
    lilo_runtime_daemon::socket::prepare_socket(&socket_path)?;
    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    fs::write(paths.pid_path(), std::process::id().to_string())
        .context("failed to write pidfile")?;

    let cancellation = CancellationToken::new();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut runtime_shutdown = runtime.subscribe_shutdown();
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("failed to accept daemon connection")?;
                let runtime = Arc::clone(&runtime);
                let session = Arc::clone(&session);
                let token = cancellation.clone();
                connections.spawn(async move {
                    if let Err(error) = handle_connection(stream, runtime, session, token).await {
                        tracing::warn!(%error, "lilod connection failed");
                    }
                });
            }
            _ = runtime_shutdown.recv() => cancellation.cancel(),
            _ = tokio::signal::ctrl_c() => cancellation.cancel(),
            _ = terminate.recv() => cancellation.cancel(),
            () = cancellation.cancelled() => break,
        }
    }

    drop(listener);
    connections.abort_all();
    while let Some(result) = connections.join_next().await {
        if let Err(error) = result {
            tracing::warn!(%error, "lilod connection task failed");
        }
    }
    lilo_runtime_daemon::socket::remove_socket_file(&socket_path)?;
    let _ = fs::remove_file(paths.pid_path());
    db.close().await;
    Ok(())
}

async fn handle_connection(
    stream: UnixStream,
    runtime: Arc<RuntimeService>,
    session: Arc<SessionService>,
    cancellation: CancellationToken,
) -> Result<()> {
    let principal = match peer_creds::extract(&stream).await {
        Ok(principal) => principal,
        Err(error) => {
            let response = RuntimeResponse::error(ErrorCode::ProtocolMismatch, error.to_string());
            let (_read_half, mut write_half) = stream.into_split();
            write_json_line(&mut write_half, &response).await?;
            return Ok(());
        }
    };

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    match read_json_line::<_, LilodRpc>(&mut reader).await {
        Ok(LilodRpc::Runtime(request)) => {
            let response = runtime.handle_rpc(principal, request).await;
            if matches!(response, RuntimeResponse::Stopping) {
                cancellation.cancel();
            }
            write_json_line(&mut write_half, &response).await?;
        }
        Ok(LilodRpc::Session(request)) => {
            let result = session.handle_rpc(principal, request).await;
            if result.shutdown {
                cancellation.cancel();
            }
            write_json_line(&mut write_half, &result.response).await?;
        }
        Err(error) => {
            let response = RpcResponse::Error {
                message: error.to_string(),
            };
            write_json_line(&mut write_half, &response).await?;
        }
    }
    Ok(())
}
