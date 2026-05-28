use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use lilo_db::LiloDb;
use lilo_paths::{LiloHome, LiloPaths, RuntimeEndpoint};
use lilo_rm_core::{
    HeadlessSpawnTarget, IsolationPolicy, Lifecycle, RuntimeEvent,
    RuntimeKind as RuntimeRuntimeKind, ShimReady, SpawnRequest as RuntimeSpawnRequest, SpawnTarget,
};
use lilo_runtime_daemon::docker_preflight::DockerPreflightConfig;
use lilo_runtime_daemon::{DaemonConfig, ReconcileConfig};
use lilo_runtime_store::StoreConfig;
use lilo_session_core::{Namespace, RuntimeKind as SessionRuntimeKind, Session, SessionState};
use tempfile::TempDir;
use uuid::Uuid;

pub struct IntegrationFixture {
    _dir: TempDir,
    pub paths: LiloPaths,
    pub db: LiloDb,
}

impl IntegrationFixture {
    pub async fn open() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let home = LiloHome::from_path(dir.path().join("lilo"))?;
        let paths = LiloPaths::new(home);
        let db = LiloDb::open(&paths).await?;
        Ok(Self {
            _dir: dir,
            paths,
            db,
        })
    }
}

pub fn runtime_config(paths: &LiloPaths) -> DaemonConfig {
    DaemonConfig {
        endpoint: RuntimeEndpoint::unix_socket(paths.socket_path()),
        shim_path: paths.run_root().join("shim"),
        log_root: paths.runtime_log_dir(Uuid::nil()),
        store: StoreConfig {
            db_path: paths.db_path(),
        },
        reconcile: ReconcileConfig {
            sweep_interval: Duration::from_secs(3_600),
            resume_poll_interval: Duration::from_secs(3_600),
            resume_gap_threshold: chrono::Duration::seconds(3),
        },
        docker_preflight: DockerPreflightConfig::default(),
    }
}

pub fn draft_session(id: Uuid) -> Session {
    let now = Utc::now();
    Session {
        id,
        runtime: SessionRuntimeKind::Claude,
        role: "worker".to_owned(),
        workspace: "/tmp".to_owned(),
        namespace: Namespace::default(),
        dir: PathBuf::from("/tmp"),
        labels: Vec::new(),
        state: SessionState::Spawning,
        runtime_pid: 0,
        runtime_session: None,
        transcript_path: None,
        tmux_pane: None,
        agent_config: None,
        created_at: now,
        started_at: now,
        terminated_at: None,
        exit_code: None,
        updated_at: now,
    }
}

pub fn runtime_request(session_id: Uuid) -> RuntimeSpawnRequest {
    RuntimeSpawnRequest {
        session_id,
        runtime: RuntimeRuntimeKind::Claude,
        isolation: IsolationPolicy::Host,
        image: None,
        env: Vec::new(),
        mounts: Vec::new(),
        cwd: PathBuf::from("/tmp"),
        target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
        force: false,
        shell_resume: None,
    }
}

pub fn running_lifecycle(session_id: Uuid) -> Lifecycle {
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeRuntimeKind::Claude);
    assert!(lifecycle.mark_running(ShimReady {
        session_id,
        shim_pid: 1,
        runtime_pid: 2,
        start_time: Utc::now(),
        tmux_pane: None,
    }));
    lifecycle
}

pub fn running_event(session_id: Uuid) -> RuntimeEvent {
    RuntimeEvent::Running {
        session_id,
        runtime_pid: 2,
        start_time: Utc::now(),
    }
}

pub fn event_log_line_count(paths: &LiloPaths) -> Result<usize> {
    match std::fs::read_to_string(paths.events_log_path()) {
        Ok(content) => Ok(content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

pub async fn count_rows(pool: &sqlx::SqlitePool, sql: &str, id: &str) -> Result<i64> {
    Ok(sqlx::query_scalar(sql).bind(id).fetch_one(pool).await?)
}

pub async fn count_all(pool: &sqlx::SqlitePool, sql: &str) -> Result<i64> {
    Ok(sqlx::query_scalar(sql).fetch_one(pool).await?)
}

pub fn fixed_uuid(suffix: u128) -> Uuid {
    Uuid::from_u128(0x018f_6e28_0000_7000_8000_0000_0000_0000 + suffix)
}
