use super::*;
use crate::identity_client::IdentityClient;
use lilo_db::LiloDb;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_rm_core::{IsolationPolicy, RuntimeKind as RuntimeRuntimeKind, ShimReady};
use lilo_runtime_daemon::{DaemonConfig, RuntimeService, RuntimeServiceContext};
use lilo_session_core::{Namespace, RuntimeKind};
use lilo_session_driver::InProcessRuntime;
use lilo_session_store::SqliteStore;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
        .complete_spawn_intent(&intent, lifecycle, event, None)
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
