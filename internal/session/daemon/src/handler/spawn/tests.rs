use super::*;
use crate::identity_client::IdentityClient;
use lilo_db::LiloDb;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_rm_core::{
    EventBatch, EventsRequest, IsolationPolicy, RuntimeKind as RuntimeRuntimeKind, ShimReady,
};
use lilo_runtime_daemon::{DaemonConfig, RuntimeService, RuntimeServiceContext};
use lilo_session_core::{Namespace, RuntimeDoctorReport, RuntimeKind};
use lilo_session_driver::{
    CaptureResult, ChildExit, InProcessRuntime, NudgeResult, RuntimeError, RuntimeFault,
    RuntimePort, SpawnedProcess,
};
use lilo_session_store::SqliteStore;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type PortFuture<'a, T> =
    Pin<Box<dyn Future<Output = std::result::Result<T, RuntimeError>> + Send + 'a>>;

#[tokio::test]
async fn namespace_deleted_recovery_kills_runtime_before_abort() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = LiloPaths::new(LiloHome::from_path(temp.path().join("lilo")).expect("home"));
    let db = LiloDb::open(&paths).await.expect("db");
    let mut config = DaemonConfig::from_lilo_paths(&paths).expect("runtime config");
    config.reconcile.sweep_interval = Duration::from_mins(1);
    config.reconcile.resume_poll_interval = Duration::from_mins(1);
    let runtime = Arc::new(
        RuntimeService::build(RuntimeServiceContext::new(config, db.clone()))
            .await
            .expect("runtime service builds"),
    );
    let runtime_port = Arc::new(InProcessRuntime::new(Arc::clone(&runtime)));
    let state = DaemonState::new(
        SqliteStore::open(&db),
        runtime_port,
        Arc::new(IdentityClient::from_db(&db, nix::unistd::getuid().as_raw())),
        Arc::clone(&runtime),
    );
    let mut child = ChildGuard::spawn(temp.path());
    let session_id = Uuid::now_v7();
    let namespace = Namespace::new("deleted").expect("namespace validates");
    let request = spawn_request(session_id, namespace, temp.path());
    let intent = pending_intent(session_id, &request);
    state
        .store
        .insert_pending_spawn_intent(&intent)
        .await
        .expect("pending intent inserts");
    let lifecycle_store = LifecycleStore::open(&db);
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeRuntimeKind::Claude);
    lifecycle_store
        .insert_forking(&lifecycle)
        .await
        .expect("forking lifecycle inserts");
    mark_running(&mut lifecycle, child.runtime_pid());
    lifecycle_store
        .update_lifecycle(&lifecycle)
        .await
        .expect("running lifecycle updates");
    let event = running_event_from_lifecycle(&lifecycle).expect("running event builds");

    let error = state
        .complete_spawn_intent(
            &intent,
            lifecycle,
            event,
            None,
            OnCommitFailure::AbortRunning,
        )
        .await
        .expect_err("namespace deleted branch fails");

    assert!(
        error
            .to_string()
            .contains("namespace deleted before session commit: deleted")
    );
    child.wait_for_exit(Duration::from_secs(2)).await;
    assert!(
        lifecycle_store
            .get(session_id)
            .await
            .expect("lifecycle lookup succeeds")
            .is_none()
    );
    runtime.shutdown().await.expect("runtime shuts down");
    db.close().await;
}

#[tokio::test]
async fn reconcile_pending_spawn_intents_continues_after_failed_intent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = LiloPaths::new(LiloHome::from_path(temp.path().join("lilo")).expect("home"));
    let db = LiloDb::open(&paths).await.expect("db");
    let runtime = Arc::new(
        RuntimeService::build(RuntimeServiceContext::new(
            DaemonConfig::from_lilo_paths(&paths).expect("runtime config"),
            db.clone(),
        ))
        .await
        .expect("runtime service builds"),
    );
    let store = SqliteStore::open(&db);
    let lifecycle_store = LifecycleStore::open(&db);
    let bad_session_id = Uuid::now_v7();
    let good_session_id = Uuid::now_v7();
    let bad_request = spawn_request(
        bad_session_id,
        Namespace::new("deleted").expect("namespace validates"),
        temp.path(),
    );
    let good_request = spawn_request(good_session_id, Namespace::default(), temp.path());
    let bad_intent = pending_intent(bad_session_id, &bad_request);
    let good_intent = pending_intent(good_session_id, &good_request);
    store
        .insert_pending_spawn_intent(&bad_intent)
        .await
        .expect("bad pending intent inserts");
    store
        .insert_pending_spawn_intent(&good_intent)
        .await
        .expect("good pending intent inserts");
    let bad_lifecycle = running_lifecycle(bad_session_id, 1001);
    let good_lifecycle = running_lifecycle(good_session_id, 1002);
    insert_running_lifecycle(&lifecycle_store, &bad_lifecycle).await;
    insert_running_lifecycle(&lifecycle_store, &good_lifecycle).await;
    let runtime_port = Arc::new(StaticStatusRuntimePort::new(vec![
        bad_lifecycle,
        good_lifecycle,
    ]));
    let state = DaemonState::new(
        store.clone(),
        runtime_port.clone(),
        Arc::new(IdentityClient::from_db(&db, nix::unistd::getuid().as_raw())),
        Arc::clone(&runtime),
    );

    state
        .reconcile_pending_spawn_intents()
        .await
        .expect("reconcile sweep completes after one intent fails");

    assert!(runtime_port.terminated(bad_session_id));
    assert!(
        store
            .get_session(&bad_session_id)
            .await
            .expect("bad session lookup succeeds")
            .is_none()
    );
    assert!(
        store
            .get_session(&good_session_id)
            .await
            .expect("good session lookup succeeds")
            .is_some()
    );
    assert!(
        store
            .list_pending_spawn_intents()
            .await
            .expect("pending intent list succeeds")
            .is_empty()
    );
    runtime.shutdown().await.expect("runtime shuts down");
    db.close().await;
}

struct ChildGuard {
    child: Child,
    runtime_pid: u32,
    exited: bool,
}

impl ChildGuard {
    fn spawn(root: &Path) -> Self {
        let pid_file = root.join("runtime.pid");
        let child = Command::new("/bin/sh")
            .arg("-c")
            .arg(r#"sleep 60 & child=$!; printf '%s' "$child" > "$1"; wait "$child""#)
            .arg("runtime-child")
            .arg(&pid_file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("child process starts");
        let mut child = child;
        let runtime_pid = read_runtime_pid(&pid_file, &mut child);
        Self {
            child,
            runtime_pid,
            exited: false,
        }
    }

    fn runtime_pid(&self) -> u32 {
        self.runtime_pid
    }

    async fn wait_for_exit(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(_status) = self.child.try_wait().expect("child status reads") {
                self.exited = true;
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("runtime child did not exit after recovery kill");
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.exited {
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg(self.runtime_pid.to_string())
                .status();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct StaticStatusRuntimePort {
    lifecycles: Vec<Lifecycle>,
    terminated_session_ids: Mutex<Vec<Uuid>>,
}

impl StaticStatusRuntimePort {
    fn new(lifecycles: Vec<Lifecycle>) -> Self {
        Self {
            lifecycles,
            terminated_session_ids: Mutex::new(Vec::new()),
        }
    }

    fn terminated(&self, session_id: Uuid) -> bool {
        self.terminated_session_ids
            .lock()
            .expect("terminated ids lock succeeds")
            .contains(&session_id)
    }
}

impl RuntimePort for StaticStatusRuntimePort {
    fn spawn<'a>(
        &'a self,
        _session_id: &'a str,
        _launch: &'a SpawnLaunch,
    ) -> PortFuture<'a, SpawnedProcess> {
        unsupported_port_call("spawn")
    }

    fn reap_exited(&self) -> PortFuture<'_, Vec<ChildExit>> {
        unsupported_port_call("reap_exited")
    }

    fn capture<'a>(
        &'a self,
        _session_id: &'a str,
        _scrollback_lines: Option<u32>,
    ) -> PortFuture<'a, CaptureResult> {
        unsupported_port_call("capture")
    }

    fn terminate<'a>(
        &'a self,
        session_id: &'a str,
        _signal: &'a str,
        _grace: Duration,
    ) -> PortFuture<'a, Option<ChildExit>> {
        Box::pin(async move {
            let session_id_uuid = Uuid::parse_str(session_id).map_err(|_| {
                RuntimeError::Fault(RuntimeFault::InvalidSessionId(session_id.to_string()))
            })?;
            self.terminated_session_ids
                .lock()
                .expect("terminated ids lock succeeds")
                .push(session_id_uuid);
            Ok(Some(ChildExit {
                session_id: session_id.to_string(),
                runtime_pid: 1001,
                exit_code: Some(143),
                transcript_path: None,
            }))
        })
    }

    fn nudge<'a>(&'a self, _session_id: &'a str, _content: &'a str) -> PortFuture<'a, NudgeResult> {
        unsupported_port_call("nudge")
    }

    fn status(&self, _filter: StatusFilter) -> PortFuture<'_, Vec<Lifecycle>> {
        Box::pin(async move { Ok(self.lifecycles.clone()) })
    }

    fn poll_events(&self, _request: EventsRequest) -> PortFuture<'_, EventBatch> {
        unsupported_port_call("poll_events")
    }

    fn doctor(&self) -> PortFuture<'_, RuntimeDoctorReport> {
        unsupported_port_call("doctor")
    }

    fn terminate_all(&self) {}
}

fn read_runtime_pid(pid_file: &Path, child: &mut Child) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Ok(contents) = std::fs::read_to_string(pid_file) {
            return contents.trim().parse().expect("runtime pid parses");
        }
        if let Some(exit) = child.try_wait().expect("child status reads") {
            panic!("child exited before runtime pid was written: {exit}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("runtime pid file was not written");
}

fn pending_intent(session_id: Uuid, request: &SpawnRequest) -> PendingSpawnIntent {
    let launch = spawn_launch(session_id, request, None);
    let runtime_request =
        runtime_spawn_request(session_id, &launch).expect("runtime request builds");
    PendingSpawnIntent::new(
        Uuid::now_v7(),
        runtime_request,
        SessionDraft::new(&draft_session(session_id, request)),
    )
}

fn draft_session(session_id: Uuid, request: &SpawnRequest) -> Session {
    let created_at = Utc::now();
    Session {
        id: session_id,
        runtime: request.runtime,
        role: request.role.clone(),
        workspace: request.workspace.clone(),
        namespace: request.namespace.clone().expect("namespace present"),
        dir: request.dir.clone().expect("dir present").into(),
        labels: Vec::new(),
        state: SessionState::Running,
        runtime_pid: 0,
        runtime_session: None,
        transcript_path: None,
        tmux_pane: None,
        agent_config: None,
        created_at,
        started_at: created_at,
        terminated_at: None,
        exit_code: None,
        updated_at: created_at,
    }
}

fn mark_running(lifecycle: &mut Lifecycle, runtime_pid: u32) {
    assert!(lifecycle.mark_running(ShimReady {
        session_id: lifecycle.session_id,
        shim_pid: runtime_pid,
        runtime_pid,
        start_time: Utc::now(),
        tmux_pane: None,
    }));
}

async fn insert_running_lifecycle(store: &LifecycleStore, lifecycle: &Lifecycle) {
    let mut forking = lifecycle.clone();
    forking.state = LifecycleState::Forking;
    store
        .insert_forking(&forking)
        .await
        .expect("forking lifecycle inserts");
    store
        .update_lifecycle(lifecycle)
        .await
        .expect("running lifecycle updates");
}

fn running_lifecycle(session_id: Uuid, runtime_pid: u32) -> Lifecycle {
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeRuntimeKind::Claude);
    lifecycle.isolation = IsolationPolicy::Host;
    mark_running(&mut lifecycle, runtime_pid);
    lifecycle
}

fn unsupported_port_call<T: Send + 'static>(operation: &'static str) -> PortFuture<'static, T> {
    Box::pin(async move {
        Err(RuntimeError::local(format!(
            "unsupported driver operation {operation}; scheduled for test"
        )))
    })
}

fn spawn_request(session_id: Uuid, namespace: Namespace, dir: &Path) -> SpawnRequest {
    SpawnRequest {
        runtime: RuntimeKind::Claude,
        role: "pm".to_string(),
        workspace: dir.display().to_string(),
        dir: Some(dir.display().to_string()),
        namespace: Some(namespace),
        target: "headless".to_string(),
        agent_config: None,
        isolation: IsolationPolicy::default(),
        image: None,
        env: vec![LaunchEnv::new("HELIOY_SESSION_ID", session_id.to_string())],
        mounts: Vec::new(),
        shell_resume: None,
        labels: Vec::new(),
        force: false,
    }
}
