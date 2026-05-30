use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use lilo_db::LiloDb;
use lilo_integration_tests::{IntegrationFixture, count_all, draft_session, running_lifecycle};
use lilo_paths::DaemonEndpoint;
use lilo_rm_core::{LifecycleState, RuntimeExit, read_json_line, write_json_line};
use lilo_runtime_store::LifecycleStore;
use lilo_session_app::compose::{self, ShutdownObserver, ShutdownStage};
use lilo_session_core::{RpcResponse, SessionRpc, SessionState};
use lilo_session_store::SqliteStore;
use lilo_wire::LilodRpc;
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::sync::{Notify, mpsc};
use uuid::Uuid;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const REAPER_TICK: Duration = Duration::from_millis(200);

#[tokio::test]
async fn compose_shutdown_paths_stop_tasks_before_db_pool() -> Result<()> {
    for trigger in [
        ShutdownTrigger::StopRpc,
        ShutdownTrigger::CtrlC,
        ShutdownTrigger::Sigterm,
    ] {
        run_shutdown_contract(trigger)
            .await
            .with_context(|| format!("{trigger:?} shutdown contract failed"))?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum ShutdownTrigger {
    StopRpc,
    CtrlC,
    Sigterm,
}

async fn run_shutdown_contract(trigger: ShutdownTrigger) -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let socket_path = fixture.paths.socket_path();
    let endpoint = DaemonEndpoint::unix_socket(socket_path.clone());
    let (stages_tx, mut stages_rx) = mpsc::unbounded_channel();
    let resume_db_close = Arc::new(Notify::new());
    let observer = ShutdownObserver::pause_before_db_close(stages_tx, Arc::clone(&resume_db_close));

    let compose_task = tokio::spawn(compose::run_with_shutdown_observer(
        fixture.paths.clone(),
        observer,
        "test-daemon",
    ));
    wait_for_accepting_socket(&socket_path).await?;
    assert_no_session_partial_rows(&fixture.db).await?;
    assert_reaper_drives_seeded_terminal_lifecycle(&fixture).await?;

    let stop_task = trigger.start_shutdown(endpoint.clone());
    let mut observed = Vec::new();
    collect_until(&mut stages_rx, &mut observed, ShutdownStage::BeforeDbClose).await?;

    trigger.assert_shutdown_started(stop_task).await?;
    assert_eq!(
        observed,
        vec![
            ShutdownStage::ListenerClosed,
            ShutdownStage::ConnectionsDrained,
            ShutdownStage::SessionTasksStopped,
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
    assert_reaper_is_quiesced_at_db_close(&fixture).await?;

    resume_db_close.notify_waiters();
    collect_until(&mut stages_rx, &mut observed, ShutdownStage::DbClosed).await?;
    tokio::time::timeout(SHUTDOWN_TIMEOUT, compose_task).await???;
    assert_eq!(observed.last(), Some(&ShutdownStage::DbClosed));
    assert_zero_count(
        fixture.db.session_pool(),
        "SELECT COUNT(*) FROM session_spawn_intents",
    )
    .await?;
    Ok(())
}

type StopTask = Option<tokio::task::JoinHandle<Result<RpcResponse>>>;

impl ShutdownTrigger {
    fn start_shutdown(self, endpoint: DaemonEndpoint) -> StopTask {
        match self {
            Self::StopRpc => Some(tokio::spawn(async move { send_shutdown(&endpoint).await })),
            Self::CtrlC => {
                signal_self(libc::SIGINT);
                None
            }
            Self::Sigterm => {
                signal_self(libc::SIGTERM);
                None
            }
        }
    }

    async fn assert_shutdown_started(self, stop_task: StopTask) -> Result<()> {
        if let Some(stop_task) = stop_task {
            let response = tokio::time::timeout(SHUTDOWN_TIMEOUT, stop_task).await???;
            assert!(matches!(response, RpcResponse::Shutdown { .. }));
        }
        Ok(())
    }
}

fn signal_self(signal: libc::c_int) {
    // SAFETY: compose has registered tokio signal handlers before this call,
    // and the signal targets the current process only.
    let result = unsafe { libc::kill(libc::getpid(), signal) };
    assert_eq!(result, 0, "failed to signal current process");
}

async fn assert_reaper_drives_seeded_terminal_lifecycle(
    fixture: &IntegrationFixture,
) -> Result<()> {
    let session_id = Uuid::now_v7();
    seed_session(fixture, session_id, SessionState::Running).await?;
    seed_terminal_runtime_lifecycle(fixture, session_id, Some(42)).await?;
    wait_for_session_state(fixture, session_id, SessionState::Terminated).await
}

async fn assert_reaper_is_quiesced_at_db_close(fixture: &IntegrationFixture) -> Result<()> {
    let session_id = Uuid::now_v7();
    seed_session(fixture, session_id, SessionState::Running).await?;
    seed_terminal_runtime_lifecycle(fixture, session_id, Some(43)).await?;
    tokio::time::sleep(REAPER_TICK + Duration::from_millis(150)).await;
    assert_session_state(fixture, session_id, SessionState::Running).await
}

async fn seed_session(
    fixture: &IntegrationFixture,
    session_id: Uuid,
    state: SessionState,
) -> Result<()> {
    let store = SqliteStore::open(&fixture.db);
    let mut session = draft_session(session_id);
    session.state = state;
    session.started_at = Utc::now();
    session.updated_at = session.started_at;
    store.insert_session(&session).await?;
    Ok(())
}

async fn seed_terminal_runtime_lifecycle(
    fixture: &IntegrationFixture,
    session_id: Uuid,
    exit_code: Option<i32>,
) -> Result<()> {
    let store = LifecycleStore::open(&fixture.db);
    let mut lifecycle = running_lifecycle(session_id);
    assert!(lifecycle.mark_exited(RuntimeExit::new(exit_code, None)));
    let mut forking = lifecycle.clone();
    forking.state = LifecycleState::Forking;
    store.insert_forking(&forking).await?;
    store.update_lifecycle(&lifecycle).await?;
    Ok(())
}

async fn wait_for_session_state(
    fixture: &IntegrationFixture,
    session_id: Uuid,
    expected: SessionState,
) -> Result<()> {
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    while Instant::now() < deadline {
        if session_state(fixture, session_id).await? == expected {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_session_state(fixture, session_id, expected).await
}

async fn assert_session_state(
    fixture: &IntegrationFixture,
    session_id: Uuid,
    expected: SessionState,
) -> Result<()> {
    let actual = session_state(fixture, session_id).await?;
    assert_eq!(actual, expected, "session {session_id} state");
    Ok(())
}

async fn session_state(fixture: &IntegrationFixture, session_id: Uuid) -> Result<SessionState> {
    let store = SqliteStore::open(&fixture.db);
    Ok(store
        .get_session(&session_id)
        .await?
        .with_context(|| format!("missing session {session_id}"))?
        .state)
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
