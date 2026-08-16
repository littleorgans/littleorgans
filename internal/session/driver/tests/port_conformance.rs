mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use common::OrPanic as _;
use lilo_common::id::SessionId;
use lilo_db::LiloDb;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_port::ParityProof;
use lilo_rm_core::{
    CaptureError, CapturePayload, CaptureRequest, CaptureResponse, CursorExpiredPayload,
    DoctorPayload, EventBatch, EventsPayload, EventsRequest, HeadlessSpawnTarget, IsolationPolicy,
    Lifecycle, LifecycleState, MountSpec, NudgeFailureReason, NudgeMode, NudgeOutcome,
    NudgePayload, NudgeRequest, NudgeResponse, RuntimeEvent, RuntimeKind, RuntimeResponse,
    RuntimeRpc, RuntimeSignal, ShellResume, ShimReady, SpawnConflictKind, SpawnConflictPayload,
    SpawnRequest, SpawnTarget, StatusFilter, StatusPayload, read_json_line, write_json_line,
};
use lilo_runtime_daemon::{DaemonConfig, ReconcileConfig, RuntimeService, RuntimeServiceContext};
use lilo_runtime_store::LifecycleStore;
use lilo_session_core::RuntimeKind as SessionRuntimeKind;
use lilo_session_driver::{
    InProcessRuntime, RuntimeError, RuntimeFault, RuntimePort, SpawnLaunch, runtime_spawn_request,
};
use lilo_wire::LilodRpc;
use tokio::io::BufReader;
use tokio::task::JoinHandle;

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn runtime_ports_map_nudge_headless_outcome_identically() {
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let in_process = in_process_fixture(session_id, LifecycleState::Running).await;
    let socket = mock_rtmd_nudge(
        session_id,
        NudgeOutcome::Unsupported(NudgeFailureReason::HeadlessLifecycle),
    );

    let direct = RuntimePort::nudge(
        &in_process.port,
        session_id,
        "hello",
        NudgeMode::Immediate,
        None,
    )
    .await
    .or_panic("in-process nudge maps");
    let via_socket = RuntimePort::nudge(
        &socket.driver,
        session_id,
        "hello",
        NudgeMode::Immediate,
        None,
    )
    .await
    .or_panic("socket nudge maps");

    assert_eq!(direct.delivered, via_socket.delivered);
    assert_eq!(direct.message, via_socket.message);
    socket.server.await.or_panic("socket server exits");
    in_process.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn runtime_ports_map_capture_headless_response_identically() {
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let in_process = in_process_fixture(session_id, LifecycleState::Running).await;
    let socket = mock_rtmd_capture(
        session_id,
        CaptureResponse::Failed(CaptureError::NotATmuxTarget),
    );

    let direct = RuntimePort::capture(&in_process.port, session_id, None)
        .await
        .or_panic("in-process capture maps");
    let via_socket = RuntimePort::capture(&socket.driver, session_id, None)
        .await
        .or_panic("socket capture maps");

    assert_eq!(direct.response, via_socket.response);
    socket.server.await.or_panic("socket server exits");
    in_process.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn runtime_ports_status_shapes_match() {
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let filter = StatusFilter::for_session(session_id);
    let in_process = in_process_fixture(session_id, LifecycleState::Running).await;

    let direct = RuntimePort::status(&in_process.port, filter.clone())
        .await
        .or_panic("in-process status maps");
    let socket = mock_rtmd_status(filter.clone(), direct.clone());
    let via_socket = RuntimePort::status(&socket.driver, filter)
        .await
        .or_panic("socket status maps");

    assert_eq!(direct, via_socket);
    socket.server.await.or_panic("socket server exits");
    in_process.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn runtime_ports_poll_events_shapes_match() {
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let in_process = in_process_fixture(session_id, LifecycleState::Running).await;
    let event = RuntimeEvent::Running {
        session_id,
        runtime_pid: 4242,
        start_time: Utc::now(),
    };
    in_process
        .runtime
        .append_event(event)
        .await
        .or_panic("append runtime event");
    let request = EventsRequest::default();

    let direct = RuntimePort::poll_events(&in_process.port, request)
        .await
        .or_panic("in-process poll events maps");
    let socket = mock_rtmd_events(request, direct.clone());
    let via_socket = RuntimePort::poll_events(&socket.driver, request)
        .await
        .or_panic("socket poll events maps");

    assert_eq!(direct, via_socket);
    socket.server.await.or_panic("socket server exits");
    in_process.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn runtime_ports_doctor_shapes_match_on_stable_fields() {
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let in_process = in_process_fixture(session_id, LifecycleState::Running).await;

    let direct = RuntimePort::doctor(&in_process.port)
        .await
        .or_panic("in-process doctor maps");
    let doctor = direct
        .doctor
        .as_ref()
        .or_panic("doctor payload present")
        .as_ref()
        .clone();
    let socket = mock_rtmd_doctor(doctor);
    let via_socket = RuntimePort::doctor(&socket.driver)
        .await
        .or_panic("socket doctor maps");

    assert_eq!(direct.status, via_socket.status);
    assert_eq!(direct.doctor, via_socket.doctor);
    assert_eq!(direct.code, via_socket.code);
    assert_eq!(direct.message, via_socket.message);
    assert!(direct.socket_path.is_none());
    assert!(via_socket.socket_path.is_some());
    socket.server.await.or_panic("socket server exits");
    in_process.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn runtime_ports_spawn_conflict_error_variant_matches() {
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let in_process = in_process_fixture(session_id, LifecycleState::Running).await;
    let request = runtime_spawn_request(spawn_launch(
        session_id,
        in_process.dir.path().to_path_buf(),
    ));
    let direct_request = request.clone();
    let (socket, received_request) = mock_rtmd_spawn_conflict(SpawnConflictPayload {
        kind: SpawnConflictKind::SessionId,
        lifecycle: running_lifecycle(session_id),
    });
    let via_socket = RuntimePort::spawn(&socket.driver, request)
        .await
        .expect_err("socket spawn conflicts");
    let received_request = received_request
        .await
        .or_panic("socket server returns decoded spawn request");
    assert_eq!(received_request, direct_request);
    let direct = RuntimePort::spawn(&in_process.port, direct_request)
        .await
        .expect_err("in-process spawn conflicts");

    assert_fault_parity(direct, via_socket);
    socket.server.await.or_panic("socket server exits");
    in_process.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn runtime_port_reap_exited_is_at_most_once() {
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let in_process = in_process_fixture(
        session_id,
        LifecycleState::Exited(lilo_rm_core::RuntimeExit::new(Some(0), None)),
    )
    .await;

    let first = RuntimePort::reap_exited(&in_process.port)
        .await
        .or_panic("first reap succeeds");
    let second = RuntimePort::reap_exited(&in_process.port)
        .await
        .or_panic("second reap succeeds");

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].session_id, session_id);
    assert!(second.is_empty());
    in_process.cleanup().await;
}

#[tokio::test]
async fn wait_for_terminal_filters_status_to_session_id() {
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let socket = mock_rtmd_kill_then_status(session_id);

    let exit = RuntimePort::terminate(
        &socket.driver,
        session_id,
        RuntimeSignal::Term,
        Duration::from_secs(1),
    )
    .await
    .or_panic("terminate waits for terminal status")
    .or_panic("terminal exit observed");

    assert_eq!(exit.session_id, session_id);
    socket.server.await.or_panic("socket server exits");
}

struct InProcessFixture {
    port: InProcessRuntime,
    runtime: Arc<RuntimeService>,
    dir: tempfile::TempDir,
    testdb: lilo_db::test_support::TestDb,
}

impl InProcessFixture {
    async fn cleanup(self) {
        self.testdb.cleanup().await.or_panic("test db cleans up");
    }
}

async fn in_process_fixture(session_id: SessionId, state: LifecycleState) -> InProcessFixture {
    let dir = tempfile::tempdir().or_panic("tempdir");
    let paths = LiloPaths::new(LiloHome::from_path(dir.path().join("lilo")).or_panic("home"));
    let testdb = lilo_db::test_support::TestDb::create()
        .await
        .or_panic("db opens");
    let db = testdb.db().clone();
    let mut config = DaemonConfig::from_lilo_paths(&paths).or_panic("runtime config");
    config.reconcile = ReconcileConfig {
        sweep_interval: Duration::from_hours(1),
        resume_poll_interval: Duration::from_hours(1),
        resume_gap_threshold: chrono::Duration::hours(1),
    };
    let runtime = Arc::new(
        RuntimeService::build(RuntimeServiceContext::new(config, db.clone()))
            .await
            .or_panic("runtime service builds"),
    );
    persist_lifecycle(&db, session_id, state).await;
    InProcessFixture {
        port: InProcessRuntime::new(Arc::clone(&runtime)),
        runtime,
        dir,
        testdb,
    }
}

async fn persist_lifecycle(db: &LiloDb, session_id: SessionId, state: LifecycleState) {
    let store = LifecycleStore::from_db(db);
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    store
        .insert_forking(&lifecycle)
        .await
        .or_panic("forking lifecycle inserts");
    lifecycle.mark_running(ShimReady {
        session_id,
        shim_pid: 1,
        runtime_pid: 2,
        start_time: Utc::now(),
        tmux_pane: None,
    });
    lifecycle.state = state;
    store
        .update_lifecycle(&lifecycle)
        .await
        .or_panic("lifecycle updates");
}

struct SocketFixture {
    driver: lilo_session_driver::RtmdDriver,
    server: JoinHandle<()>,
}

fn mock_rtmd_nudge(session_id: SessionId, outcome: NudgeOutcome) -> SocketFixture {
    mock_rtmd_once(
        RuntimeRpc::Nudge {
            request: NudgeRequest {
                session_id,
                content: "hello".to_string(),
                mode: NudgeMode::Immediate,
                timeout_ms: None,
            },
        },
        RuntimeResponse::Nudge(NudgePayload {
            response: NudgeResponse {
                delivered: matches!(outcome, NudgeOutcome::Delivered),
                outcome,
            },
        }),
    )
}

fn mock_rtmd_capture(session_id: SessionId, response: CaptureResponse) -> SocketFixture {
    mock_rtmd_once(
        RuntimeRpc::Capture {
            request: CaptureRequest {
                session_id,
                scrollback_lines: None,
            },
        },
        RuntimeResponse::Capture(CapturePayload { response }),
    )
}

fn mock_rtmd_status(filter: StatusFilter, lifecycles: Vec<Lifecycle>) -> SocketFixture {
    mock_rtmd_once(
        RuntimeRpc::Status {
            request: filter.into(),
        },
        RuntimeResponse::Status(StatusPayload { lifecycles }),
    )
}

fn mock_rtmd_events(request: EventsRequest, batch: EventBatch) -> SocketFixture {
    mock_rtmd_once(RuntimeRpc::Events { request }, event_batch_response(batch))
}

fn mock_rtmd_doctor(doctor: lilo_rm_core::DoctorResponse) -> SocketFixture {
    mock_rtmd_once(
        RuntimeRpc::Doctor,
        RuntimeResponse::Doctor(DoctorPayload { doctor }),
    )
}

fn mock_rtmd_spawn_conflict(
    conflict: SpawnConflictPayload,
) -> (SocketFixture, tokio::sync::oneshot::Receiver<SpawnRequest>) {
    let (request_tx, request_rx) = tokio::sync::oneshot::channel();
    let (driver, server) = common::mock_rtmd_server(move |stream| async move {
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let envelope: LilodRpc = read_json_line(&mut reader).await.or_panic("read rpc");
        let LilodRpc::Runtime(RuntimeRpc::Spawn { request }) = envelope else {
            panic!("expected runtime spawn rpc");
        };
        request_tx
            .send(*request)
            .or_panic("return decoded spawn request");
        write_json_line(&mut write_half, &RuntimeResponse::SpawnConflict(conflict))
            .await
            .or_panic("write runtime response");
    });
    (SocketFixture { driver, server }, request_rx)
}

fn mock_rtmd_once(expected: RuntimeRpc, response: RuntimeResponse) -> SocketFixture {
    let (driver, server) = common::mock_rtmd_server(move |stream| async move {
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let envelope: LilodRpc = read_json_line(&mut reader).await.or_panic("read rpc");
        assert_eq!(envelope, LilodRpc::Runtime(expected));
        write_json_line(&mut write_half, &response)
            .await
            .or_panic("write runtime response");
    });
    SocketFixture { driver, server }
}

fn event_batch_response(batch: EventBatch) -> RuntimeResponse {
    match batch {
        EventBatch::Events { events, cursor } => {
            RuntimeResponse::Events(EventsPayload { events, cursor })
        }
        EventBatch::CursorExpired { oldest } => {
            RuntimeResponse::CursorExpired(CursorExpiredPayload { oldest })
        }
    }
}

fn mock_rtmd_kill_then_status(session_id: SessionId) -> SocketFixture {
    let tempdir = tempfile::tempdir().or_panic("tempdir");
    let socket_path = tempdir.path().join("rtmd.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).or_panic("bind test socket");
    let driver = lilo_session_driver::RtmdDriver::new(socket_path);
    let server = tokio::spawn(async move {
        let _tempdir = tempdir;
        let (stream, _) = listener.accept().await.or_panic("accept kill client");
        handle_kill(stream, session_id).await;
        let (stream, _) = listener.accept().await.or_panic("accept status client");
        handle_status(stream, session_id).await;
    });
    SocketFixture { driver, server }
}

async fn handle_kill(stream: tokio::net::UnixStream, session_id: SessionId) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let envelope: LilodRpc = read_json_line(&mut reader).await.or_panic("read kill rpc");
    assert_eq!(
        envelope,
        LilodRpc::Runtime(RuntimeRpc::Kill {
            request: lilo_rm_core::KillRequest {
                session_id,
                signal: RuntimeSignal::Term,
                grace_secs: 1,
            },
        })
    );
    write_json_line(
        &mut write_half,
        &RuntimeResponse::Killed(lilo_rm_core::KilledPayload {
            outcome: lilo_rm_core::KillOutcome::Signalled,
        }),
    )
    .await
    .or_panic("write kill response");
}

async fn handle_status(stream: tokio::net::UnixStream, session_id: SessionId) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let envelope: LilodRpc = read_json_line(&mut reader)
        .await
        .or_panic("read status rpc");
    assert_eq!(
        envelope,
        LilodRpc::Runtime(RuntimeRpc::Status {
            request: StatusFilter {
                session_id: Some(session_id),
                session_ids: Vec::new(),
                updated_since: None,
                runtime: None,
                state: None,
            }
            .into(),
        })
    );
    write_json_line(
        &mut write_half,
        &RuntimeResponse::Status(StatusPayload {
            lifecycles: vec![exited_lifecycle(session_id)],
        }),
    )
    .await
    .or_panic("write status response");
}

fn running_lifecycle(session_id: SessionId) -> Lifecycle {
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    lifecycle.mark_running(ShimReady {
        session_id,
        shim_pid: 1,
        runtime_pid: 2,
        start_time: Utc::now(),
        tmux_pane: None,
    });
    lifecycle
}

fn spawn_launch(session_id: SessionId, cwd: PathBuf) -> SpawnLaunch {
    SpawnLaunch {
        session_id,
        runtime: SessionRuntimeKind::Claude,
        isolation: IsolationPolicy::default(),
        image: None,
        cwd,
        target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
        env: Vec::new(),
        mounts: Vec::<MountSpec>::new(),
        shell_resume: None::<ShellResume>,
        force: false,
        launch_attachment: Some(common::shared_test_support::launch_attachment_fixture()),
    }
}

fn assert_fault_parity(direct: RuntimeError, via_socket: RuntimeError) -> ParityProof {
    let direct = runtime_fault("in-process", direct);
    let via_socket = runtime_fault("socket", via_socket);
    match &direct {
        RuntimeFault::SpawnConflict { .. } => lilo_port::prove_eq(&direct, &via_socket),
    }
}

fn runtime_fault(adapter: &str, error: RuntimeError) -> RuntimeFault {
    let RuntimeError::Fault(fault) = error else {
        panic!("expected {adapter} runtime fault, got {error:?}");
    };
    fault
}

fn exited_lifecycle(session_id: SessionId) -> Lifecycle {
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    lifecycle.mark_running(ShimReady {
        session_id,
        shim_pid: 1,
        runtime_pid: 2,
        start_time: Utc::now(),
        tmux_pane: None,
    });
    lifecycle.state = LifecycleState::Exited(lilo_rm_core::RuntimeExit::new(Some(0), None));
    lifecycle
}
