use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, TimeZone};
use lilo_rm_core::{IsolationPolicy, IsolationProfile, LifecycleState, RuntimeKind, ShimReady};
use lilo_runtime_store::{LifecycleStore, StoreConfig};
use uuid::Uuid;

use super::*;
use crate::server::DaemonConfig;

struct FakeProbe {
    alive: HashSet<u32>,
    start_times: HashMap<u32, DateTime<Utc>>,
}

impl ProcessProbe for FakeProbe {
    fn pid_alive(&self, pid: u32) -> bool {
        self.alive.contains(&pid)
    }

    fn start_time_for_pid(&self, pid: u32) -> Result<ProcessStartTime> {
        Ok(self
            .start_times
            .get(&pid)
            .copied()
            .map_or(ProcessStartTime::Unsupported, ProcessStartTime::Known))
    }
}

struct VanishingProbe {
    alive_checks: AtomicUsize,
}

impl ProcessProbe for VanishingProbe {
    fn pid_alive(&self, _pid: u32) -> bool {
        self.alive_checks.fetch_add(1, Ordering::SeqCst) == 0
    }

    fn start_time_for_pid(&self, pid: u32) -> Result<ProcessStartTime> {
        Err(anyhow!("failed to read start time for pid {pid}"))
    }
}

struct GoneProbe;

impl ProcessProbe for GoneProbe {
    fn pid_alive(&self, _pid: u32) -> bool {
        true
    }

    fn start_time_for_pid(&self, _pid: u32) -> Result<ProcessStartTime> {
        Ok(ProcessStartTime::Gone)
    }
}

struct FakeDockerLiveness {
    checks: AtomicUsize,
    evidence: Option<LostEvidence>,
}

impl DockerLiveness for FakeDockerLiveness {
    fn lost_evidence(&self, _lifecycle: &Lifecycle) -> Result<Option<LostEvidence>> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        Ok(self.evidence)
    }
}

#[tokio::test]
async fn startup_reconciliation_marks_dead_and_reused_pids_lost_once() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let store = open_store(temp.path().join("rtm.sqlite")).await;
    let dead = persist_running(&store, 101, Utc.timestamp_opt(1_000, 0).unwrap()).await;
    let reused = persist_running(&store, 202, Utc.timestamp_opt(2_000, 0).unwrap()).await;
    let mut already_lost = persist_running(&store, 303, Utc.timestamp_opt(3_000, 0).unwrap()).await;
    already_lost.mark_lost(LostEvidence::PidNotAlive);
    store
        .update_lifecycle(&already_lost)
        .await
        .expect("persist lost");

    let state = Arc::new(ServerState::new(test_config(temp.path()), store.clone()).expect("state"));
    let probe = FakeProbe {
        alive: HashSet::from([202]),
        start_times: HashMap::from([(202, Utc.timestamp_opt(2_001, 0).unwrap())]),
    };

    let events = reconcile_startup(Arc::clone(&state), &probe)
        .await
        .expect("reconcile");
    let replay = reconcile_startup(state, &probe).await.expect("replay");

    assert_eq!(events.len(), 2);
    assert!(replay.is_empty(), "{replay:?}");
    assert_lost(&store, dead.session_id, LostEvidence::PidNotAlive).await;
    assert_lost(&store, reused.session_id, LostEvidence::PidReuseDetected).await;
}

#[tokio::test]
async fn startup_reconciliation_marks_pid_lost_when_start_time_races_exit() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let store = open_store(temp.path().join("rtm.sqlite")).await;
    let lifecycle = persist_running(&store, 404, Utc.timestamp_opt(4_000, 0).unwrap()).await;
    let state = Arc::new(ServerState::new(test_config(temp.path()), store.clone()).expect("state"));
    let probe = VanishingProbe {
        alive_checks: AtomicUsize::new(0),
    };

    let events = reconcile_startup(state, &probe).await.expect("reconcile");

    assert_eq!(events.len(), 1);
    assert_lost(&store, lifecycle.session_id, LostEvidence::PidNotAlive).await;
}

#[tokio::test]
async fn startup_reconciliation_marks_pid_lost_when_probe_reports_gone() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let store = open_store(temp.path().join("rtm.sqlite")).await;
    let lifecycle = persist_running(&store, 505, Utc.timestamp_opt(5_000, 0).unwrap()).await;
    let state = Arc::new(ServerState::new(test_config(temp.path()), store.clone()).expect("state"));

    let events = reconcile_startup(state, &GoneProbe)
        .await
        .expect("reconcile");

    assert_eq!(events.len(), 1);
    assert_lost(&store, lifecycle.session_id, LostEvidence::PidNotAlive).await;
}

#[tokio::test]
async fn periodic_reconciliation_marks_dead_and_reused_pids_lost_once() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let store = open_store(temp.path().join("rtm.sqlite")).await;
    let dead = persist_running(&store, 101, Utc.timestamp_opt(1_000, 0).unwrap()).await;
    let reused = persist_running(&store, 202, Utc.timestamp_opt(2_000, 0).unwrap()).await;
    let state = Arc::new(ServerState::new(test_config(temp.path()), store.clone()).expect("state"));
    let probe = FakeProbe {
        alive: HashSet::from([202]),
        start_times: HashMap::from([(202, Utc.timestamp_opt(2_001, 0).unwrap())]),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
    let task = tokio::spawn(run_periodic_with_config(
        Arc::clone(&state),
        probe,
        shutdown_rx,
        ReconcileConfig {
            sweep_interval: Duration::from_millis(20),
            ..ReconcileConfig::default()
        },
    ));

    wait_for_events(&state, 2).await;
    let _ = shutdown_tx.send(());
    task.await.expect("periodic task");

    assert_lost(&store, dead.session_id, LostEvidence::PidNotAlive).await;
    assert_lost(&store, reused.session_id, LostEvidence::PidReuseDetected).await;
    assert_eq!(
        state
            .events(lilo_rm_core::EventsRequest {
                since: Some(0),
                wait_ms: None
            })
            .await
            .expect("events")
            .events
            .len(),
        2
    );
    assert_eq!(store.running().await.expect("running").len(), 0);
}

#[tokio::test]
async fn docker_reconciliation_uses_docker_liveness() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let store = open_store(temp.path().join("rtm.sqlite")).await;
    let lifecycle = persist_docker_running(&store, 606).await;
    let state = Arc::new(ServerState::new(test_config(temp.path()), store.clone()).expect("state"));
    let process = FakeProbe {
        alive: HashSet::new(),
        start_times: HashMap::new(),
    };
    let docker = FakeDockerLiveness {
        checks: AtomicUsize::new(0),
        evidence: Some(LostEvidence::PidNotAlive),
    };
    let liveness = ReconcileLiveness {
        process: &process,
        docker: &docker,
    };

    let events = reconcile_once(state, &liveness).await.expect("reconcile");

    assert_eq!(docker.checks.load(Ordering::SeqCst), 1);
    assert_eq!(events.len(), 1);
    assert_lost(&store, lifecycle.session_id, LostEvidence::PidNotAlive).await;
}

#[test]
fn resume_gap_threshold_detects_wall_clock_jump() {
    let last_wall_tick = Utc.timestamp_opt(1_000, 0).unwrap();
    let wall_now = Utc.timestamp_opt(1_004, 0).unwrap();

    assert!(wall_now - last_wall_tick > RESUME_GAP_THRESHOLD);
}

async fn assert_lost(store: &LifecycleStore, session_id: Uuid, evidence: LostEvidence) {
    let lifecycle = store
        .get(session_id)
        .await
        .expect("get")
        .expect("lifecycle");
    assert_eq!(lifecycle.state, LifecycleState::Lost(evidence));
}

async fn persist_running(store: &LifecycleStore, pid: u32, start_time: DateTime<Utc>) -> Lifecycle {
    let mut lifecycle = forking_lifecycle();
    store.insert_forking(&lifecycle).await.expect("insert");
    lifecycle.mark_running(ShimReady {
        session_id: lifecycle.session_id,
        shim_pid: pid + 10_000,
        runtime_pid: pid,
        start_time,
        tmux_pane: None,
    });
    store.update_lifecycle(&lifecycle).await.expect("running");
    lifecycle
}

async fn persist_docker_running(store: &LifecycleStore, pid: u32) -> Lifecycle {
    let mut lifecycle = forking_lifecycle();
    lifecycle.isolation = IsolationPolicy::Docker(IsolationProfile {
        name: Some("locked".to_owned()),
    });
    store.insert_forking(&lifecycle).await.expect("insert");
    lifecycle.mark_running(ShimReady {
        session_id: lifecycle.session_id,
        shim_pid: pid + 10_000,
        runtime_pid: pid,
        start_time: Utc.timestamp_opt(6_000, 0).unwrap(),
        tmux_pane: None,
    });
    store.update_lifecycle(&lifecycle).await.expect("running");
    lifecycle
}

fn forking_lifecycle() -> Lifecycle {
    let session_id = Uuid::now_v7();
    Lifecycle::forking(session_id, RuntimeKind::Claude)
}

async fn wait_for_events(state: &ServerState, expected: usize) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if state
                .events(lilo_rm_core::EventsRequest {
                    since: Some(0),
                    wait_ms: None,
                })
                .await
                .expect("events")
                .events
                .len()
                == expected
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("events reached expected count");
}

fn test_config(root: &Path) -> DaemonConfig {
    DaemonConfig {
        endpoint: lilo_paths::RuntimeEndpoint::unix_socket(root.join("rtm-test.sock")),
        shim_path: PathBuf::from("rtm"),
        log_root: root.join("logs"),
        store: StoreConfig {
            db_path: root.join("rtm-test.sqlite"),
        },
        reconcile: ReconcileConfig::default(),
        docker_preflight: crate::docker_preflight::DockerPreflightConfig::default(),
        tmux_server_label: None,
    }
}

async fn open_store(path: impl AsRef<Path>) -> LifecycleStore {
    let db = lilo_db::LiloDb::open_path(path).await.expect("store db");
    LifecycleStore::open(&db)
}
