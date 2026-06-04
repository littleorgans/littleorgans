mod common;

use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use common::{OrPanic, mail_request};
use lilo_common::id::SessionId;
use lilo_db::LiloDb;
use lilo_paths::{DaemonEndpoint, LiloHome, LiloPaths};
use lilo_session_core::{
    CallerContextRequest, MailIntent, MailLogFilter, MailTailRequest, Namespace, RpcResponse,
    RuntimeKind, Selector, Session, SessionRpc, SessionState,
};
use lilo_session_daemon::run_daemon_with_db;
use lilo_session_store::SqliteStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const THREAD: &str = "serve-follow-thread";

#[tokio::test]
async fn follow_tail_does_not_block_concurrent_mail_send() {
    let fixture = ServerFixture::new().await;
    let sender = fixture.insert_session("pm").await;
    let recipient = fixture.insert_session("engineer").await;
    let daemon = fixture.spawn_daemon().await;

    let tail_endpoint = fixture.endpoint.clone();
    let tail = tokio::spawn(async move {
        send_raw_request(
            &tail_endpoint,
            SessionRpc::MailTail {
                request: MailTailRequest {
                    filter: log_filter(),
                    after: None,
                    follow: true,
                    wait_ms: Some(5_000),
                },
            },
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let sent = tokio::time::timeout(
        Duration::from_secs(2),
        send_raw_request(
            &fixture.endpoint,
            caller_request(
                sender.id,
                SessionRpc::MailSend {
                    request: mail_request(
                        Selector::Id { id: recipient.id },
                        "observe me",
                        THREAD,
                        MailIntent::Request,
                    ),
                },
            ),
        ),
    )
    .await
    .or_panic("mail send completes while tail follows")
    .or_panic("mail send response");
    assert!(matches!(sent, RpcResponse::MailSent { .. }));

    let tailed = tokio::time::timeout(Duration::from_secs(2), tail)
        .await
        .or_panic("tail completes after append")
        .or_panic("tail task joins")
        .or_panic("tail response");
    let RpcResponse::MailTail { response } = tailed else {
        panic!("expected mail tail response");
    };
    assert_eq!(response.messages.len(), 1);
    assert_eq!(response.messages[0].content, "observe me");

    fixture.shutdown(daemon).await;
}

#[tokio::test]
async fn malformed_connection_does_not_stop_accept_loop() {
    let fixture = ServerFixture::new().await;
    fixture.insert_session("engineer").await;
    let daemon = fixture.spawn_daemon().await;

    send_malformed_request(&fixture.endpoint).await;
    let response = send_raw_request(
        &fixture.endpoint,
        SessionRpc::MailTail {
            request: MailTailRequest {
                filter: log_filter(),
                after: None,
                follow: false,
                wait_ms: None,
            },
        },
    )
    .await
    .or_panic("daemon accepts after malformed request");
    assert!(matches!(response, RpcResponse::MailTail { .. }));

    fixture.shutdown(daemon).await;
}

#[tokio::test]
async fn shutdown_ack_is_written_before_daemon_exits() {
    let fixture = ServerFixture::new().await;
    let daemon = fixture.spawn_daemon().await;

    let response = send_raw_request(&fixture.endpoint, SessionRpc::Shutdown)
        .await
        .or_panic("shutdown response");
    assert!(matches!(response, RpcResponse::Shutdown { .. }));
    tokio::time::timeout(Duration::from_secs(2), daemon)
        .await
        .or_panic("daemon exits after shutdown")
        .or_panic("daemon task joins")
        .or_panic("daemon exits cleanly");
}

struct ServerFixture {
    _dir: tempfile::TempDir,
    paths: LiloPaths,
    endpoint: DaemonEndpoint,
    db: LiloDb,
    store: SqliteStore,
}

impl ServerFixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().or_panic("tempdir creates");
        let paths = LiloPaths::new(
            LiloHome::from_path(dir.path().join("lilo")).or_panic("lilo home resolves"),
        );
        let db = LiloDb::open(&paths).await.or_panic("db opens");
        let store = SqliteStore::open(&db);
        let endpoint = DaemonEndpoint::from_paths(&paths);
        Self {
            _dir: dir,
            paths,
            endpoint,
            db,
            store,
        }
    }

    async fn insert_session(&self, role: &str) -> Session {
        let now = Utc::now();
        let session = Session {
            id: SessionId::new(),
            runtime: RuntimeKind::Codex,
            role: role.to_string(),
            workspace: "/tmp/lilo-test".to_string(),
            namespace: Namespace::default(),
            dir: Path::new("/tmp/lilo-test").to_path_buf(),
            labels: Vec::new(),
            state: SessionState::Running,
            runtime_pid: 1,
            runtime_session: None,
            transcript_path: None,
            tmux_pane: None,
            agent_config: None,
            created_at: now,
            started_at: now,
            terminated_at: None,
            exit_code: None,
            updated_at: now,
        };
        self.store
            .insert_session(&session)
            .await
            .or_panic("session inserts");
        session
    }

    async fn spawn_daemon(&self) -> tokio::task::JoinHandle<anyhow::Result<()>> {
        let paths = self.paths.clone();
        let db = self.db.clone();
        let endpoint = self.endpoint.clone();
        let daemon =
            tokio::spawn(async move { run_daemon_with_db(paths, "test-daemon", db).await });
        wait_for_socket(&endpoint).await;
        daemon
    }

    async fn shutdown(&self, daemon: tokio::task::JoinHandle<anyhow::Result<()>>) {
        let response = send_raw_request(&self.endpoint, SessionRpc::Shutdown)
            .await
            .or_panic("shutdown response");
        assert!(matches!(response, RpcResponse::Shutdown { .. }));
        tokio::time::timeout(Duration::from_secs(2), daemon)
            .await
            .or_panic("daemon exits after shutdown")
            .or_panic("daemon task joins")
            .or_panic("daemon exits cleanly");
    }
}

async fn send_raw_request(
    endpoint: &DaemonEndpoint,
    request: SessionRpc,
) -> anyhow::Result<RpcResponse> {
    let mut stream = lilo_sys::ipc::connect(endpoint.as_path()).await?;
    stream.write_all(&serde_json::to_vec(&request)?).await?;
    stream.shutdown().await?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn send_malformed_request(endpoint: &DaemonEndpoint) {
    let mut stream = lilo_sys::ipc::connect(endpoint.as_path())
        .await
        .or_panic("malformed request connects");
    stream
        .write_all(b"{")
        .await
        .or_panic("malformed request writes");
    stream
        .shutdown()
        .await
        .or_panic("malformed request closes write half");
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .await
        .or_panic("malformed response reads");
    let response: RpcResponse = serde_json::from_slice(&bytes).or_panic("error response decodes");
    assert!(matches!(response, RpcResponse::Error { .. }));
}

async fn wait_for_socket(endpoint: &DaemonEndpoint) {
    for _ in 0..50 {
        if endpoint.as_path().exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("daemon socket did not appear");
}

fn caller_request(caller_session_id: SessionId, request: SessionRpc) -> SessionRpc {
    SessionRpc::CallerContext {
        request: CallerContextRequest {
            caller_session_id: caller_session_id.to_string(),
            request: Box::new(request),
        },
    }
}

fn log_filter() -> MailLogFilter {
    MailLogFilter {
        context_id: Some(THREAD.to_string()),
        selector: None,
        recipient: None,
        include_system: false,
    }
}
