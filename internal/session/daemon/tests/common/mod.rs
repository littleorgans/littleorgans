#![allow(dead_code)]

use lilo_db::LiloDb;
use lilo_im_core::{AuditRow, Principal};
use lilo_paths::{LiloHome, LiloPaths};
use lilo_rm_core::{
    DoctorPayload, LifecycleCounts, MigrationState, RuntimeResponse, RuntimeRpc, TmuxStatus,
    WatcherCounts, read_json_line, version_info, write_json_line,
};
use lilo_runtime_daemon::{DaemonConfig, RuntimeService, RuntimeServiceContext};
use lilo_session_core::{
    IsolationPolicy, Label, MailCheckRequest, Namespace, RpcResponse, RuntimeKind, Selector,
    Session, SessionRpc, SpawnRequest,
};
use lilo_session_daemon::handler::{DaemonState, HandlerResult};
use lilo_session_daemon::identity_client::{IdentityClient, RequestContext};
use lilo_session_driver::{LaunchEnv, RtmdDriver};
use lilo_session_store::SqliteStore;
use lilo_wire::LilodRpc;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;
use uuid::Uuid;

#[path = "../../../test_support.rs"]
pub mod shared_test_support;
pub use shared_test_support::OrPanic;

pub const LOCAL_UID: u32 = 42;

pub struct TestDaemon {
    pub state: DaemonState,
    pub audit_path: PathBuf,
    pub dir: tempfile::TempDir,
    pub runtime: Arc<RuntimeService>,
    runtime_socket_task: JoinHandle<()>,
}

impl TestDaemon {
    pub async fn new(local_uid: u32) -> Self {
        let dir = tempfile::tempdir().or_panic("tempdir creates");
        warm_runtime_launchers_with_fake_runtime();
        let paths = LiloPaths::new(
            LiloHome::from_path(dir.path().join("lilo")).or_panic("lilo home resolves"),
        );
        std::fs::create_dir_all(paths.run_root()).or_panic("run dir creates");
        let db = LiloDb::open(&paths).await.or_panic("store db opens");
        let audit_path = paths.db_path();
        let identity = IdentityClient::new(
            lilo_im_store::SqliteAuditSink::with_pool(db.identity_pool().clone()),
            local_uid,
        );
        let store = SqliteStore::open(&db);
        let mut runtime_config =
            DaemonConfig::from_lilo_paths(&paths).or_panic("runtime config resolves");
        runtime_config.shim_path = assert_cmd::cargo::cargo_bin("lilo");
        let runtime = Arc::new(
            RuntimeService::build(RuntimeServiceContext::new_with_local_uid(
                runtime_config,
                db.clone(),
                local_uid,
            ))
            .await
            .or_panic("runtime service builds"),
        );
        let runtime_socket_path = paths.socket_path();
        let driver = Arc::new(RtmdDriver::new(runtime_socket_path.clone()));
        let runtime_socket_task = spawn_runtime_socket(&runtime_socket_path, Arc::clone(&runtime));
        let state = DaemonState::new(store, driver, Arc::new(identity), Arc::clone(&runtime))
            .with_rtmd_socket_path(runtime_socket_path);
        Self {
            state,
            audit_path,
            dir,
            runtime,
            runtime_socket_task,
        }
    }

    pub async fn audit_rows(&self) -> Vec<AuditRow> {
        let db = lilo_db::LiloDb::open_path(&self.audit_path)
            .await
            .or_panic("audit db opens");
        lilo_im_store::query_audit(db.identity_pool(), lilo_im_store::AuditFilters::default())
            .await
            .or_panic("audit query succeeds")
    }
}

fn spawn_runtime_socket(socket_path: &Path, runtime: Arc<RuntimeService>) -> JoinHandle<()> {
    let listener = UnixListener::bind(socket_path).or_panic("runtime socket binds");
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.or_panic("runtime socket accepts");
            let runtime = Arc::clone(&runtime);
            tokio::spawn(async move {
                handle_runtime_socket_connection(stream, runtime).await;
            });
        }
    })
}

async fn handle_runtime_socket_connection(stream: UnixStream, runtime: Arc<RuntimeService>) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let response = match read_json_line::<_, LilodRpc>(&mut reader).await {
        Ok(LilodRpc::Runtime(request)) => {
            runtime
                .handle_rpc(Principal::Local(LOCAL_UID), request)
                .await
        }
        Ok(LilodRpc::Session(_)) => RuntimeResponse::error(
            lilo_rm_core::ErrorCode::ProtocolMismatch,
            "session rpc reached runtime test socket",
        ),
        Err(error) => {
            RuntimeResponse::error(lilo_rm_core::ErrorCode::ProtocolMismatch, error.to_string())
        }
    };
    write_json_line(&mut write_half, &response)
        .await
        .or_panic("runtime socket writes response");
}

fn warm_runtime_launchers_with_fake_runtime() {
    let _guard = launcher_env_lock().lock().or_panic("launcher env lock");
    let original_path = std::env::var_os("PATH");
    let path = test_path(fake_runtime_dir());
    // SAFETY: Launcher warmup serializes PATH mutation through launcher_env_lock.
    unsafe { std::env::set_var("PATH", path) };
    let result = lilo_runtime_launchers::warm_registry();
    match original_path {
        Some(path) => {
            // SAFETY: Launcher warmup serializes PATH mutation through launcher_env_lock.
            unsafe { std::env::set_var("PATH", path) };
        }
        None => {
            // SAFETY: Launcher warmup serializes PATH mutation through launcher_env_lock.
            unsafe { std::env::remove_var("PATH") };
        }
    }
    result.or_panic("runtime launchers warm");
}

fn launcher_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn fake_runtime_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().or_panic("fake runtime tempdir creates");
        write_fake_runtime(dir.path(), "claude");
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);
        path
    })
}

fn write_fake_runtime(dir: &Path, name: &str) {
    let path = dir.join(name);
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf 'lilo fake runtime ready\\n'\nexec sleep 60\n",
    )
    .or_panic("fake runtime writes");
    let mut permissions = std::fs::metadata(&path).or_panic("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).or_panic("permissions");
}

fn test_path(fake_bin_dir: &Path) -> std::ffi::OsString {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![fake_bin_dir.to_path_buf()];
    paths.extend(std::env::split_paths(&current));
    std::env::join_paths(paths).or_panic("PATH can be joined")
}

pub async fn spawn_test_session(
    daemon: &TestDaemon,
    context: &RequestContext,
    role: &str,
) -> Session {
    spawn_test_session_with_labels(daemon, context, role, Vec::new()).await
}

pub async fn spawn_test_session_with_labels(
    daemon: &TestDaemon,
    context: &RequestContext,
    role: &str,
    labels: Vec<Label>,
) -> Session {
    let mut request = headless_spawn_request(role, daemon.dir.path().display().to_string());
    request.labels = labels;
    let spawned = handle_spawn(daemon, context.clone(), request).await;
    let RpcResponse::Spawned { response } = spawned.response else {
        panic!("expected spawn response");
    };
    response.session
}

pub async fn handle_spawn(
    daemon: &TestDaemon,
    context: RequestContext,
    request: SpawnRequest,
) -> HandlerResult {
    daemon
        .state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(request),
            },
        )
        .await
}

pub fn headless_spawn_request(role: &str, workspace: impl Into<String>) -> SpawnRequest {
    spawn_request(role, workspace, "headless")
}

pub fn namespace_spawn_request(
    role: &str,
    dir: impl Into<String>,
    namespace: Namespace,
) -> SpawnRequest {
    let mut request = headless_spawn_request(role, String::new());
    request.dir = Some(dir.into());
    request.namespace = Some(namespace);
    request
}

pub fn spawn_request(
    role: &str,
    workspace: impl Into<String>,
    target: impl Into<String>,
) -> SpawnRequest {
    SpawnRequest {
        runtime: RuntimeKind::Claude,
        role: role.to_string(),
        workspace: workspace.into(),
        dir: None,
        namespace: None,
        target: target.into(),
        agent_config: None,
        isolation: IsolationPolicy::default(),
        image: None,
        env: Vec::new(),
        mounts: Vec::new(),
        shell_resume: None,
        labels: Vec::new(),
        force: false,
    }
}

pub async fn mail_count(state: &DaemonState, context: RequestContext, session_id: Uuid) -> usize {
    let response = state
        .handle(
            context,
            SessionRpc::MailCheck {
                request: MailCheckRequest {
                    selector: Selector::Id { id: session_id },
                },
            },
        )
        .await;
    let RpcResponse::MailChecked { response } = response.response else {
        panic!("expected mail check response");
    };
    response.unread
}

pub fn mock_rtmd_doctor(doctor: lilo_rm_core::DoctorResponse) -> (PathBuf, JoinHandle<()>) {
    let tempdir = tempfile::tempdir().or_panic("tempdir creates");
    let socket_path = tempdir.path().join("rtmd.sock");
    let listener = UnixListener::bind(&socket_path).or_panic("rtmd test socket binds");
    let server = tokio::spawn(async move {
        let _tempdir = tempdir;
        respond_to_rtmd_status(&listener).await;
        let mut rpc = read_rtmd_rpc(&listener).await;
        assert_eq!(rpc.0, RuntimeRpc::Doctor);
        write_json_line(
            &mut rpc.1,
            &RuntimeResponse::Doctor(DoctorPayload { doctor }),
        )
        .await
        .or_panic("write rtmd doctor response");
    });
    (socket_path, server)
}

async fn respond_to_rtmd_status(listener: &UnixListener) {
    let mut rpc = read_rtmd_rpc(listener).await;
    let RuntimeRpc::Status { .. } = rpc.0 else {
        panic!("expected status rpc before doctor");
    };
    write_json_line(
        &mut rpc.1,
        &RuntimeResponse::Status(lilo_rm_core::StatusPayload {
            lifecycles: Vec::new(),
        }),
    )
    .await
    .or_panic("write rtmd status response");
}

async fn read_rtmd_rpc(listener: &UnixListener) -> (RuntimeRpc, tokio::net::unix::OwnedWriteHalf) {
    let (stream, _) = listener.accept().await.or_panic("accept rtmd client");
    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let envelope: LilodRpc = read_json_line(&mut reader).await.or_panic("read rtmd rpc");
    let LilodRpc::Runtime(rpc) = envelope else {
        panic!("expected runtime rpc");
    };
    (rpc, write_half)
}

pub fn runtime_doctor_response() -> lilo_rm_core::DoctorResponse {
    lilo_rm_core::DoctorResponse {
        version: version_info(),
        socket_path: "/tmp/rtmd.sock".to_string(),
        uptime_secs: 7,
        sqlite: MigrationState {
            applied: 1,
            total: 1,
            applied_descriptions: vec!["init".to_string()],
            pending_descriptions: Vec::new(),
        },
        lifecycles: LifecycleCounts {
            running: 1,
            ..LifecycleCounts::default()
        },
        watchers: WatcherCounts {
            process_exit_watchers: 1,
            shim_sockets: 0,
            event_waiters: 0,
        },
        launchers: Vec::new(),
        tmux: TmuxStatus {
            available: false,
            version: None,
            error: Some("tmux unavailable in test".to_string()),
        },
        docker: Box::new(lilo_rm_core::DockerStatus::legacy_missing()),
        log_availability: Vec::new(),
        last_probe_sweep: None,
        recent_lost: Vec::new(),
    }
}

pub fn local_context() -> RequestContext {
    RequestContext::new(Principal::Local(LOCAL_UID))
}

pub fn launch_env(key: &str, value: &str) -> LaunchEnv {
    LaunchEnv {
        key: key.to_string(),
        value: value.to_string(),
    }
}
