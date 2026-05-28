use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lilo_rm_client::{ClientError, EventWatcher, RuntimeClient};
use lilo_rm_core::{
    EventBatch, EventsRequest, ProtocolError, RUNTIME_PROTOCOL_VERSION, RuntimeResponse,
    RuntimeRpc, VersionInfo, VersionPayload, read_json_line, write_json_line,
};
use lilo_runtime_daemon::{
    DaemonConfig, ReconcileConfig, docker_preflight::DockerPreflightConfig, run_daemon,
};
use lilo_runtime_store::StoreConfig;
use lilo_wire::LilodRpc;
use serde_json::json;
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;
use uuid::Uuid;

struct TestDaemon {
    client: RuntimeClient,
    task: JoinHandle<()>,
    tempdir: tempfile::TempDir,
}

impl TestDaemon {
    async fn start_with_log(records: &[serde_json::Value]) -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let socket_path = tempdir.path().join("rtmd.sock");
        write_event_log(tempdir.path(), records);
        let config = DaemonConfig {
            endpoint: lilo_paths::RuntimeEndpoint::unix_socket(socket_path.clone()),
            shim_path: std::env::current_exe().expect("current test executable"),
            log_root: tempdir.path().join("logs"),
            store: StoreConfig {
                db_path: tempdir.path().join("rtm.sqlite"),
            },
            reconcile: ReconcileConfig::default(),
            docker_preflight: DockerPreflightConfig::default(),
        };
        let task = tokio::spawn(async move {
            run_daemon(config).await.expect("daemon run");
        });
        wait_for_socket(&socket_path).await;
        Self {
            client: RuntimeClient::new(socket_path),
            task,
            tempdir,
        }
    }

    fn client(&self) -> RuntimeClient {
        self.client.clone()
    }

    async fn stop(self) {
        let response = self
            .client
            .request(RuntimeRpc::Stop)
            .await
            .expect("stop daemon");
        assert_eq!(response, RuntimeResponse::Stopping);
        self.task.await.expect("daemon task");
        drop(self.tempdir);
    }
}

async fn wait_for_socket(socket_path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_error = None;
    while Instant::now() < deadline {
        match UnixStream::connect(socket_path).await {
            Ok(_) => return,
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
    panic!(
        "daemon socket never accepted connections at {}; last error={last_error:?}",
        socket_path.display()
    );
}

#[tokio::test]
async fn connect_rejects_protocol_mismatch() {
    let (client, server) = mock_version_client("0.3");

    let error = EventWatcher::builder()
        .connect(client)
        .await
        .expect_err("protocol mismatch should fail before polling");

    match error {
        ClientError::Protocol {
            source: ProtocolError::UnsupportedVersion { expected, got },
        } => {
            assert_eq!(expected, RUNTIME_PROTOCOL_VERSION);
            assert_eq!(got, "0.3");
        }
        other => panic!("unexpected client error: {other:?}"),
    }
    server.await.expect("server task");
}

#[tokio::test]
async fn connect_accepts_matching_protocol() {
    let (client, server) = mock_version_client(RUNTIME_PROTOCOL_VERSION);

    let watcher = EventWatcher::builder()
        .since(7)
        .connect(client)
        .await
        .expect("matching protocol should connect");

    assert_eq!(watcher.current_cursor(), Some(&7));
    server.await.expect("server task");
}

#[tokio::test]
async fn next_uses_default_wait_ms() {
    let request = next_request(EventWatcher::builder()).await;

    assert_eq!(
        request,
        EventsRequest {
            since: None,
            wait_ms: Some(30_000)
        }
    );
}

#[tokio::test]
async fn next_uses_configured_wait_ms_and_seek_cursor() {
    let request = next_request(EventWatcher::builder().since(3).wait_ms(25)).await;

    assert_eq!(
        request,
        EventsRequest {
            since: Some(3),
            wait_ms: Some(25)
        }
    );
}

#[tokio::test]
async fn cursor_durability_survives_watcher_rebuild() {
    let daemon = TestDaemon::start_with_log(&[event_record(1), event_record(2)]).await;
    let mut watcher = EventWatcher::builder()
        .wait_ms(0)
        .connect(daemon.client())
        .await
        .expect("connect watcher");

    let first = watcher.next().await.expect("first batch");
    assert_event_count(&first, 2);
    let persisted = *watcher.current_cursor().expect("persisted cursor");
    drop(watcher);

    let mut rebuilt = EventWatcher::builder()
        .since(persisted)
        .wait_ms(0)
        .connect(daemon.client())
        .await
        .expect("reconnect watcher");
    let second = rebuilt.next().await.expect("resumed batch");

    assert_event_count(&second, 0);
    assert_eq!(rebuilt.current_cursor(), Some(&persisted));
    daemon.stop().await;
}

#[tokio::test]
async fn cursor_expired_advances_cursor_and_can_resume_from_oldest() {
    let daemon = TestDaemon::start_with_log(&[event_record(3)]).await;
    let mut watcher = EventWatcher::builder()
        .since(0)
        .wait_ms(0)
        .connect(daemon.client())
        .await
        .expect("connect watcher");

    let expired = watcher.next().await.expect("expired cursor batch");
    assert_eq!(expired, EventBatch::CursorExpired { oldest: 2 });
    assert_eq!(watcher.current_cursor(), Some(&2));

    let resumed = watcher.next().await.expect("resumed batch");
    assert_event_count(&resumed, 1);
    assert_eq!(watcher.current_cursor(), Some(&3));
    daemon.stop().await;
}

#[tokio::test]
async fn seek_repositions_next_request() {
    let daemon = TestDaemon::start_with_log(&[event_record(1), event_record(2)]).await;
    let mut watcher = EventWatcher::builder()
        .since(2)
        .wait_ms(0)
        .connect(daemon.client())
        .await
        .expect("connect watcher");

    watcher.seek(1);
    let batch = watcher.next().await.expect("seek batch");

    assert_event_count(&batch, 1);
    assert_eq!(watcher.current_cursor(), Some(&2));
    daemon.stop().await;
}

fn mock_version_client(protocol_version: &str) -> (RuntimeClient, JoinHandle<()>) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let socket_path = tempdir.path().join("rtmd.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind test socket");
    let client = RuntimeClient::new(socket_path);
    let mut version = VersionInfo::new("0.6.0", "test-sha");
    protocol_version.clone_into(&mut version.protocol_version);
    let server = tokio::spawn(async move {
        let _tempdir = tempdir;
        let (stream, _) = listener.accept().await.expect("accept client");
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let envelope: LilodRpc = read_json_line(&mut reader).await.expect("read rpc");
        let LilodRpc::Runtime(rpc) = envelope else {
            panic!("expected runtime rpc");
        };
        assert_eq!(rpc, RuntimeRpc::Version);
        write_json_line(
            &mut write_half,
            &RuntimeResponse::Version(VersionPayload { version }),
        )
        .await
        .expect("write response");
    });
    (client, server)
}

async fn next_request(builder: lilo_rm_client::EventWatcherBuilder) -> EventsRequest {
    let (tempdir, socket_path) = temp_socket_path();
    let listener = UnixListener::bind(&socket_path).expect("bind test socket");
    let client = RuntimeClient::new(socket_path);
    let server = tokio::spawn(async move {
        let _tempdir = tempdir;
        let (stream, _) = listener.accept().await.expect("accept client");
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let envelope: LilodRpc = read_json_line(&mut reader).await.expect("read rpc");
        let LilodRpc::Runtime(rpc) = envelope else {
            panic!("expected runtime rpc");
        };
        let RuntimeRpc::Events { request } = rpc else {
            panic!("expected events rpc");
        };
        write_json_line(
            &mut write_half,
            &RuntimeResponse::Events(lilo_rm_core::EventsPayload {
                events: Vec::new(),
                cursor: request.since.unwrap_or_default(),
            }),
        )
        .await
        .expect("write response");
        request
    });
    let mut watcher = builder.build(client);
    watcher.next().await.expect("watcher next");
    server.await.expect("server task")
}

fn temp_socket_path() -> (tempfile::TempDir, PathBuf) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let socket_path = tempdir.path().join("rtmd.sock");
    (tempdir, socket_path)
}

fn assert_event_count(batch: &EventBatch, expected: usize) {
    match batch {
        EventBatch::Events { events, .. } => assert_eq!(events.len(), expected),
        other => panic!("expected events batch, got {other:?}"),
    }
}

fn write_event_log(root: &Path, records: &[serde_json::Value]) {
    let path = lilo_paths::event_log_path(root);
    create_dir_all(path.parent().expect("event log parent")).expect("event log dir");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)
        .expect("event log");
    for record in records {
        writeln!(file, "{record}").expect("record");
    }
}

fn event_record(seq: u64) -> serde_json::Value {
    json!({
        "seq": seq,
        "ts_ms": 1_700_000_000_000_u64,
        "kind": "running",
        "payload": {
            "session_id": Uuid::now_v7(),
            "runtime_pid": 4242,
            "start_time": "2023-11-14T22:13:20Z"
        }
    })
}
