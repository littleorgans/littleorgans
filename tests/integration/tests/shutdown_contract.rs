use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use lilo_db::LiloDb;
use lilo_integration_tests::{IntegrationFixture, count_all};
use lilo_paths::DaemonEndpoint;
use lilo_rm_core::{read_json_line, write_json_line};
use lilo_session_app::compose::{self, ShutdownObserver, ShutdownStage};
use lilo_session_core::{RpcResponse, SessionRpc};
use lilo_wire::LilodRpc;
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::sync::{Notify, mpsc};

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn compose_shutdown_rpc_closes_listener_before_db_pool() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let socket_path = fixture.paths.socket_path();
    let endpoint = DaemonEndpoint::unix_socket(socket_path.clone());
    let (stages_tx, mut stages_rx) = mpsc::unbounded_channel();
    let resume_db_close = Arc::new(Notify::new());
    let observer = ShutdownObserver::pause_before_db_close(stages_tx, Arc::clone(&resume_db_close));

    let compose_task = tokio::spawn(compose::run_with_shutdown_observer(
        fixture.paths.clone(),
        observer,
    ));
    wait_for_accepting_socket(&socket_path).await?;

    let stop_endpoint = endpoint.clone();
    let stop_task = tokio::spawn(async move { send_shutdown(&stop_endpoint).await });
    let mut observed = Vec::new();
    collect_until(&mut stages_rx, &mut observed, ShutdownStage::BeforeDbClose).await?;

    let response = tokio::time::timeout(SHUTDOWN_TIMEOUT, stop_task).await???;
    assert!(matches!(response, RpcResponse::Shutdown { .. }));
    assert_eq!(
        observed,
        vec![
            ShutdownStage::ListenerClosed,
            ShutdownStage::ConnectionsDrained,
            ShutdownStage::RuntimeShutdown,
            ShutdownStage::SocketRemoved,
            ShutdownStage::BeforeDbClose,
        ]
    );
    assert_socket_rejects(&socket_path).await?;
    assert!(
        !compose_task.is_finished(),
        "compose closed the DB before the test released it"
    );
    assert_no_session_partial_rows(&fixture.db).await?;

    resume_db_close.notify_waiters();
    collect_until(&mut stages_rx, &mut observed, ShutdownStage::DbClosed).await?;
    tokio::time::timeout(SHUTDOWN_TIMEOUT, compose_task).await???;
    assert_eq!(observed.last(), Some(&ShutdownStage::DbClosed));
    assert_no_session_partial_rows(&fixture.db).await?;
    Ok(())
}

async fn send_shutdown(endpoint: &DaemonEndpoint) -> Result<RpcResponse> {
    let stream = UnixStream::connect(endpoint.as_path())
        .await
        .with_context(|| format!("failed to connect to {endpoint}"))?;
    let (read_half, mut write_half) = stream.into_split();
    write_json_line(&mut write_half, &LilodRpc::Session(SessionRpc::Shutdown))
        .await
        .context("failed to write shutdown request")?;
    let mut reader = BufReader::new(read_half);
    read_json_line(&mut reader)
        .await
        .context("failed to read shutdown response")
}

async fn collect_until(
    stages_rx: &mut mpsc::UnboundedReceiver<ShutdownStage>,
    observed: &mut Vec<ShutdownStage>,
    expected: ShutdownStage,
) -> Result<()> {
    loop {
        let stage = tokio::time::timeout(SHUTDOWN_TIMEOUT, stages_rx.recv())
            .await
            .context("timed out waiting for shutdown stage")?
            .context("compose exited before reporting shutdown stage")?;
        observed.push(stage);
        if stage == expected {
            return Ok(());
        }
    }
}

async fn wait_for_accepting_socket(path: &Path) -> Result<()> {
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    while Instant::now() < deadline {
        match UnixStream::connect(path).await {
            Ok(_) => return Ok(()),
            Err(error) if is_socket_not_ready(&error) => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(error) => return Err(error).context("failed while waiting for compose socket"),
        }
    }
    bail!(
        "compose socket did not accept connections at {}",
        path.display()
    )
}

async fn assert_socket_rejects(path: &Path) -> Result<()> {
    match UnixStream::connect(path).await {
        Ok(_) => bail!("compose listener accepted a connection after shutdown started"),
        Err(error) if is_socket_not_ready(&error) => Ok(()),
        Err(error) => Err(error).context("unexpected socket error after listener close"),
    }
}

fn is_socket_not_ready(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::NotFound | ErrorKind::ConnectionRefused
    )
}

async fn assert_no_session_partial_rows(db: &LiloDb) -> Result<()> {
    assert_zero_count(
        db.session_pool(),
        "SELECT COUNT(*) FROM session_spawn_intents",
    )
    .await?;
    assert_zero_count(db.session_pool(), "SELECT COUNT(*) FROM session_sessions").await?;
    assert_zero_count(db.runtime_pool(), "SELECT COUNT(*) FROM runtime_lifecycle").await?;
    Ok(())
}

async fn assert_zero_count(pool: &sqlx::SqlitePool, sql: &str) -> Result<()> {
    let count = count_all(pool, sql).await?;
    assert_eq!(count, 0, "{sql}");
    Ok(())
}
