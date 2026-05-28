#[path = "common/daemon.rs"]
mod daemon;
#[path = "common/mock_socket.rs"]
mod mock_socket;

use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;
use std::path::Path;

use lilo_rm_client::{ClientError, EventWatcher, RuntimeClient};
use lilo_rm_core::{
    EventBatch, EventsRequest, ProtocolError, RUNTIME_PROTOCOL_VERSION, RuntimeResponse,
    RuntimeRpc, VersionInfo, VersionPayload,
};
use mock_socket::{mock_runtime_exchange, mock_runtime_response};
use serde_json::json;
use tokio::task::JoinHandle;
use uuid::Uuid;

use daemon::TestDaemon;

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
    let daemon = TestDaemon::start_with_data(|root| {
        write_event_log(root, &[event_record(1), event_record(2)]);
    })
    .await;
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
    let daemon = TestDaemon::start_with_data(|root| {
        write_event_log(root, &[event_record(3)]);
    })
    .await;
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
    let daemon = TestDaemon::start_with_data(|root| {
        write_event_log(root, &[event_record(1), event_record(2)]);
    })
    .await;
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
    let mut version = VersionInfo::new("0.6.0", "test-sha");
    protocol_version.clone_into(&mut version.protocol_version);
    mock_runtime_response(
        RuntimeRpc::Version,
        RuntimeResponse::Version(VersionPayload { version }),
    )
}

async fn next_request(builder: lilo_rm_client::EventWatcherBuilder) -> EventsRequest {
    let (client, server) = mock_runtime_exchange(|rpc| {
        let RuntimeRpc::Events { request } = rpc else {
            panic!("expected events rpc");
        };
        let response = RuntimeResponse::Events(lilo_rm_core::EventsPayload {
            events: Vec::new(),
            cursor: request.since.unwrap_or_default(),
        });
        (response, request)
    });
    let mut watcher = builder.build(client);
    watcher.next().await.expect("watcher next");
    server.await.expect("server task")
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
