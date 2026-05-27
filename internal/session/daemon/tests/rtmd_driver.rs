mod common;

use common::OrPanic as _;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use lilo_im_core::Principal;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_rm_client::RuntimeClient;
use lilo_rm_core::{Lifecycle, LifecycleState, StatusFilter};
use lilo_session_core::{
    DeleteRequest, IsolationPolicy, LogsRequest, RpcResponse, RuntimeKind, Selector, Session,
    SessionRpc, SessionState, SpawnRequest,
};
use lilo_session_daemon::handler::DaemonState;
use lilo_session_daemon::identity_client::{IdentityClient, RequestContext};
use lilo_session_driver::RtmdDriver;
use lilo_session_store::SqliteStore;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use uuid::Uuid;

#[tokio::test]
async fn rtmd_driver_spawn_is_visible_to_sm_and_rtmd() {
    let rtm = rtm_binary();
    let temp = tempfile::tempdir().or_panic("tempdir");
    write_fake_runtime(temp.path(), "claude");
    let mut rtmd = RtmdHarness::start(&rtm, temp.path());

    let identity = IdentityClient::connect(&temp.path().join("audit.sqlite"), 42)
        .await
        .or_panic("identity connects");
    let state = DaemonState::new(
        open_temp_store().await,
        std::sync::Arc::new(RtmdDriver::new(rtmd.socket.clone())),
        std::sync::Arc::new(identity),
    );
    let context = RequestContext::new(Principal::Local(42));

    let session = spawn_session(&state, context, temp.path()).await;
    assert_eq!(session.runtime, RuntimeKind::Claude);
    let transcript_path = session
        .transcript_path
        .as_deref()
        .or_panic("transcript path");
    assert!(
        wait_for_log_content(transcript_path)
            .await
            .contains("rtm fake runtime ready")
    );
    assert!(
        logs_session(
            &state,
            RequestContext::new(Principal::Local(42)),
            session.id
        )
        .await
        .contains("rtm fake runtime ready")
    );
    assert_eq!(session.runtime_pid, runtime_pid(&rtmd, session.id).await);

    rtmd.stop();
}

#[tokio::test]
async fn rtmd_driver_delete_signalled_session_marks_terminated() {
    let rtm = rtm_binary();
    let temp = tempfile::tempdir().or_panic("tempdir");
    write_fake_runtime(temp.path(), "claude");
    let mut rtmd = RtmdHarness::start(&rtm, temp.path());
    let state = rtmd_state(&rtmd, temp.path()).await;
    let context = RequestContext::new(Principal::Local(42));
    let session = spawn_session(&state, context.clone(), temp.path()).await;

    let deleted = delete_session(&state, context, session.id, 2).await;

    assert_eq!(deleted.state, SessionState::Terminated);
    assert!(matches!(
        runtime_lifecycle(&rtmd, session.id).await.state,
        LifecycleState::Exited(_)
    ));

    rtmd.stop();
}

#[tokio::test]
async fn rtmd_driver_delete_already_exited_session_marks_terminated() {
    let rtm = rtm_binary();
    let temp = tempfile::tempdir().or_panic("tempdir");
    write_fake_runtime(temp.path(), "claude");
    let mut rtmd = RtmdHarness::start(&rtm, temp.path());
    let state = rtmd_state(&rtmd, temp.path()).await;
    let context = RequestContext::new(Principal::Local(42));
    let session = spawn_session(&state, context.clone(), temp.path()).await;

    let runtime_pid = i32::try_from(session.runtime_pid).or_panic("runtime pid fits i32");
    kill(Pid::from_raw(runtime_pid), Signal::SIGKILL).or_panic("runtime process can be killed");
    wait_for_runtime_exit(&rtmd, session.id).await;
    let deleted = delete_session(&state, context, session.id, 2).await;

    assert_eq!(deleted.state, SessionState::Terminated);

    rtmd.stop();
}

async fn rtmd_state(rtmd: &RtmdHarness, dir: &Path) -> DaemonState {
    let identity = IdentityClient::connect(&dir.join("audit.sqlite"), 42)
        .await
        .or_panic("identity connects");
    DaemonState::new(
        open_temp_store().await,
        std::sync::Arc::new(RtmdDriver::new(rtmd.socket.clone())),
        std::sync::Arc::new(identity),
    )
}

async fn open_temp_store() -> SqliteStore {
    let dir = tempfile::tempdir().or_panic("store tempdir creates");
    let db = lilo_db::LiloDb::open_path(dir.path().join("lilo.db"))
        .await
        .or_panic("store db opens");
    let store = SqliteStore::open(&db);
    std::mem::forget(dir);
    store
}

async fn spawn_session(state: &DaemonState, context: RequestContext, workspace: &Path) -> Session {
    let result = state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "engineer".to_string(),
                    workspace: workspace.display().to_string(),
                    dir: None,
                    namespace: None,
                    target: "headless".to_string(),
                    agent_config: None,
                    isolation: IsolationPolicy::default(),
                    image: None,
                    env: Vec::new(),
                    mounts: Vec::new(),
                    shell_resume: None,
                    labels: Vec::new(),
                    force: false,
                }),
            },
        )
        .await;
    let RpcResponse::Spawned { response } = result.response else {
        panic!("expected spawn response");
    };
    response.session
}

async fn delete_session(
    state: &DaemonState,
    context: RequestContext,
    id: Uuid,
    grace_secs: u64,
) -> Session {
    let deleted = state
        .handle(
            context,
            SessionRpc::Delete {
                request: DeleteRequest {
                    selector: Selector::Id { id },
                    signal: "SIGTERM".to_string(),
                    grace_secs,
                },
            },
        )
        .await;
    let RpcResponse::Deleted { response } = deleted.response else {
        panic!("expected delete response");
    };
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    response
        .sessions
        .into_iter()
        .next()
        .or_panic("deleted session")
}

async fn logs_session(state: &DaemonState, context: RequestContext, id: Uuid) -> String {
    let logs = state
        .handle(
            context,
            SessionRpc::Logs {
                request: LogsRequest {
                    selector: Selector::Id { id },
                    max_bytes: None,
                },
            },
        )
        .await;
    let RpcResponse::Logs { response } = logs.response else {
        panic!("expected logs response");
    };
    response.content
}

struct RtmdHarness {
    lilo: PathBuf,
    socket: PathBuf,
    home: PathBuf,
    child: Child,
}

impl RtmdHarness {
    fn start(lilo: &Path, dir: &Path) -> Self {
        let home = dir.join("lilo-home");
        let paths = LiloPaths::new(LiloHome::from_path(home.clone()).or_panic("lilo home"));
        let socket = paths.socket_path();
        let mut child = Command::new(lilo)
            .arg("daemon")
            .arg("start")
            .env("LILO_HOME", &home)
            .env("PATH", test_path(dir))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .or_panic("lilod starts");
        wait_for_socket(&socket, &mut child);
        Self {
            lilo: lilo.to_path_buf(),
            socket,
            home,
            child,
        }
    }

    fn stop(&mut self) {
        let _ = Command::new(&self.lilo)
            .arg("daemon")
            .arg("stop")
            .env("LILO_HOME", &self.home)
            .output();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for RtmdHarness {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn runtime_pid(harness: &RtmdHarness, session_id: Uuid) -> u32 {
    let lifecycle = runtime_lifecycle(harness, session_id).await;
    assert!(matches!(
        lifecycle.state,
        LifecycleState::Forking | LifecycleState::Running
    ));
    lifecycle.runtime_pid.or_panic("runtime pid")
}

async fn runtime_lifecycle(harness: &RtmdHarness, session_id: Uuid) -> Lifecycle {
    let payload = RuntimeClient::new(harness.socket.clone())
        .status(StatusFilter {
            session_id: Some(session_id),
            session_ids: Vec::new(),
            updated_since: None,
            runtime: None,
            state: None,
        })
        .await
        .or_panic("rtmd status");
    payload
        .lifecycles
        .into_iter()
        .find(|lifecycle| lifecycle.session_id == session_id)
        .or_panic("rtmd lifecycle exists")
}

async fn wait_for_runtime_exit(harness: &RtmdHarness, session_id: Uuid) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if matches!(
            runtime_lifecycle(harness, session_id).await.state,
            LifecycleState::Exited(_)
        ) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("runtime lifecycle did not exit");
}

async fn wait_for_log_content(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(content) = std::fs::read_to_string(path)
            && !content.is_empty()
        {
            return content;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("runtime log did not receive content at {}", path.display());
}

fn rtm_binary() -> PathBuf {
    std::env::var_os("LILO_TEST_BIN")
        .map_or_else(|| assert_cmd::cargo::cargo_bin("lilo"), PathBuf::from)
}

fn write_fake_runtime(dir: &Path, name: &str) {
    let path = dir.join(name);
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf 'rtm fake runtime ready\\n'\nexec sleep 60\n",
    )
    .or_panic("fake runtime writes");
    let mut permissions = std::fs::metadata(&path).or_panic("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).or_panic("permissions");
}

fn test_path(dir: &Path) -> String {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let paths = std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&current));
    std::env::join_paths(paths)
        .or_panic("joined path")
        .to_string_lossy()
        .into_owned()
}

fn wait_for_socket(socket: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if UnixStream::connect(socket).is_ok() {
            return;
        }
        if child.try_wait().or_panic("rtmd try_wait").is_some() {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            panic!("lilod exited before socket appeared: {stderr}");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("rtmd socket never appeared at {}", socket.display());
}
