mod common;

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use common::OrPanic as _;
use lilo_db::LiloDb;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_rm_core::{
    CaptureError, CapturePayload, CaptureRequest, CaptureResponse, Lifecycle, LifecycleState,
    NudgeFailureReason, NudgeOutcome, NudgePayload, NudgeRequest, NudgeResponse, RuntimeKind,
    RuntimeResponse, RuntimeRpc, RuntimeSignal, ShimReady, StatusFilter, StatusPayload,
    read_json_line, write_json_line,
};
use lilo_runtime_daemon::{DaemonConfig, ReconcileConfig, RuntimeService, RuntimeServiceContext};
use lilo_runtime_store::LifecycleStore;
use lilo_session_driver::{InProcessRuntime, RuntimePort};
use lilo_wire::LilodRpc;
use tokio::io::BufReader;
use tokio::task::JoinHandle;
use uuid::Uuid;

#[tokio::test]
async fn runtime_ports_map_nudge_headless_outcome_identically() {
    let session_id = Uuid::now_v7();
    let in_process = in_process_fixture(session_id, LifecycleState::Running).await;
    let socket = mock_rtmd_nudge(
        session_id,
        NudgeOutcome::Unsupported(NudgeFailureReason::HeadlessLifecycle),
    );

    let direct = RuntimePort::nudge(&in_process.port, &session_id.to_string(), "hello")
        .await
        .or_panic("in-process nudge maps");
    let via_socket = RuntimePort::nudge(&socket.driver, &session_id.to_string(), "hello")
        .await
        .or_panic("socket nudge maps");

    assert_eq!(direct.delivered, via_socket.delivered);
    assert_eq!(direct.message, via_socket.message);
    socket.server.await.or_panic("socket server exits");
}

#[tokio::test]
async fn runtime_ports_map_capture_headless_response_identically() {
    let session_id = Uuid::now_v7();
    let in_process = in_process_fixture(session_id, LifecycleState::Running).await;
    let socket = mock_rtmd_capture(
        session_id,
        CaptureResponse::Failed(CaptureError::NotATmuxTarget),
    );

    let direct = RuntimePort::capture(&in_process.port, &session_id.to_string(), None)
        .await
        .or_panic("in-process capture maps");
    let via_socket = RuntimePort::capture(&socket.driver, &session_id.to_string(), None)
        .await
        .or_panic("socket capture maps");

    assert_eq!(direct.response, via_socket.response);
    socket.server.await.or_panic("socket server exits");
}

#[tokio::test]
async fn runtime_port_reap_exited_is_at_most_once() {
    let session_id = Uuid::now_v7();
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
    assert_eq!(first[0].session_id, session_id.to_string());
    assert!(second.is_empty());
}

#[tokio::test]
async fn wait_for_terminal_filters_status_to_session_id() {
    let session_id = Uuid::now_v7();
    let socket = mock_rtmd_kill_then_status(session_id);

    let exit = RuntimePort::terminate(
        &socket.driver,
        &session_id.to_string(),
        "term",
        Duration::from_secs(1),
    )
    .await
    .or_panic("terminate waits for terminal status")
    .or_panic("terminal exit observed");

    assert_eq!(exit.session_id, session_id.to_string());
    socket.server.await.or_panic("socket server exits");
}

struct InProcessFixture {
    port: InProcessRuntime,
    _dir: tempfile::TempDir,
}

async fn in_process_fixture(session_id: Uuid, state: LifecycleState) -> InProcessFixture {
    let dir = tempfile::tempdir().or_panic("tempdir");
    let paths = LiloPaths::new(LiloHome::from_path(dir.path().join("lilo")).or_panic("home"));
    let db = LiloDb::open(&paths).await.or_panic("db opens");
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
        port: InProcessRuntime::new(runtime),
        _dir: dir,
    }
}

async fn persist_lifecycle(db: &LiloDb, session_id: Uuid, state: LifecycleState) {
    let store = LifecycleStore::open(db);
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

fn mock_rtmd_nudge(session_id: Uuid, outcome: NudgeOutcome) -> SocketFixture {
    let (driver, server) = common::mock_rtmd_server(move |stream| async move {
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let envelope: LilodRpc = read_json_line(&mut reader).await.or_panic("read rpc");
        assert_eq!(
            envelope,
            LilodRpc::Runtime(RuntimeRpc::Nudge {
                request: NudgeRequest {
                    session_id,
                    content: "hello".to_string(),
                },
            })
        );
        write_json_line(
            &mut write_half,
            &RuntimeResponse::Nudge(NudgePayload {
                response: NudgeResponse {
                    delivered: matches!(outcome, NudgeOutcome::Delivered),
                    outcome,
                },
            }),
        )
        .await
        .or_panic("write nudge response");
    });
    SocketFixture { driver, server }
}

fn mock_rtmd_capture(session_id: Uuid, response: CaptureResponse) -> SocketFixture {
    let (driver, server) = common::mock_rtmd_server(move |stream| async move {
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let envelope: LilodRpc = read_json_line(&mut reader).await.or_panic("read rpc");
        assert_eq!(
            envelope,
            LilodRpc::Runtime(RuntimeRpc::Capture {
                request: CaptureRequest {
                    session_id,
                    scrollback_lines: None,
                },
            })
        );
        write_json_line(
            &mut write_half,
            &RuntimeResponse::Capture(CapturePayload { response }),
        )
        .await
        .or_panic("write capture response");
    });
    SocketFixture { driver, server }
}

fn mock_rtmd_kill_then_status(session_id: Uuid) -> SocketFixture {
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

async fn handle_kill(stream: tokio::net::UnixStream, session_id: Uuid) {
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

async fn handle_status(stream: tokio::net::UnixStream, session_id: Uuid) {
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

fn exited_lifecycle(session_id: Uuid) -> Lifecycle {
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
