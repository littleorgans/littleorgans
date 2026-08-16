use std::time::Duration;

use anyhow::{Context, Result};
use lilo_common::id::SessionId;
use lilo_integration_tests::{IntegrationFixture, draft_session, wait_for_accepting_socket};
use lilo_paths::DaemonEndpoint;
use lilo_rm_client::RuntimeClient;
use lilo_rm_core::{RUNTIME_PROTOCOL_VERSION, read_json_line};
use lilo_session_app::compose;
use lilo_session_core::{
    CallerContextRequest, MailIntent, MailLogFilter, MailSendRequest, MailTailRequest, RpcResponse,
    Selector, SessionRpc, SessionState,
};
use lilo_session_daemon::{send_request, send_request_with_timeout};
use lilo_session_store::SessionStore;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::task::JoinHandle;

const THREAD: &str = "serve-follow-thread";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn composed_lilod_preserves_connection_contracts() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    // This cross-package test cannot live in `lilo-session-daemon`, which
    // forbids unsafe code. The integration package can own the process
    // environment without reversing production dependency edges.
    //
    // SAFETY: this test target has one top level test. It sets the variable
    // before spawning compose and never changes it during the process.
    unsafe {
        std::env::set_var("LILO_DATABASE_URL", fixture.database_url());
    }

    let endpoint = DaemonEndpoint::from_paths(&fixture.paths);
    let store = SessionStore::from_db(&fixture.db);
    let sender = insert_running_session(&store, "pm").await?;
    let recipient = insert_running_session(&store, "engineer").await?;
    let compose_task = tokio::spawn(compose::run(fixture.paths.clone(), "test-daemon"));
    wait_for_accepting_socket(&fixture.paths.socket_path()).await?;

    follow_tail_does_not_block_concurrent_mail_send(&endpoint, sender, recipient).await?;
    malformed_connection_does_not_stop_accept_loop(&endpoint).await?;
    assert_runtime_route(&fixture.paths.socket_path()).await?;
    shutdown_composed_host(&endpoint, compose_task).await?;
    fixture.cleanup().await
}

async fn insert_running_session(store: &SessionStore, role: &str) -> Result<SessionId> {
    let id = SessionId::new();
    let mut session = draft_session(id);
    session.role = role.to_owned();
    session.state = SessionState::Running;
    session.runtime_pid = 1;
    store.insert_session(&session).await?;
    Ok(id)
}

async fn follow_tail_does_not_block_concurrent_mail_send(
    endpoint: &DaemonEndpoint,
    sender: SessionId,
    recipient: SessionId,
) -> Result<()> {
    let tail_endpoint = endpoint.clone();
    let tail = tokio::spawn(async move {
        send_request(
            &tail_endpoint,
            &SessionRpc::MailTail {
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

    let request = caller_request(
        sender,
        SessionRpc::MailSend {
            request: MailSendRequest {
                to: Selector::Id { id: recipient },
                content: "observe me".to_owned(),
                notify: None,
                timeout_ms: None,
                context_id: THREAD.to_owned(),
                intent: MailIntent::Request,
                idempotency_key: None,
            },
        },
    );
    let sent = send_request_with_timeout(endpoint, &request, REQUEST_TIMEOUT)
        .await
        .context("mail send completes while tail follows")?;
    assert!(matches!(sent, RpcResponse::MailSent { .. }));

    let tailed = tokio::time::timeout(REQUEST_TIMEOUT, tail)
        .await
        .context("tail completes after append")???;
    let RpcResponse::MailTail { response } = tailed else {
        anyhow::bail!("expected mail tail response");
    };
    assert_eq!(response.messages.len(), 1);
    assert_eq!(response.messages[0].content, "observe me");
    Ok(())
}

async fn malformed_connection_does_not_stop_accept_loop(endpoint: &DaemonEndpoint) -> Result<()> {
    let mut stream = lilo_sys::ipc::connect(endpoint.as_path()).await?;
    stream.write_all(b"{").await?;
    stream.shutdown().await?;
    let mut reader = BufReader::new(stream);
    let response: RpcResponse = read_json_line(&mut reader).await?;
    assert!(matches!(response, RpcResponse::Error { .. }));

    let response = send_request(
        endpoint,
        &SessionRpc::MailTail {
            request: MailTailRequest {
                filter: log_filter(),
                after: None,
                follow: false,
                wait_ms: None,
            },
        },
    )
    .await
    .context("composed host accepts after malformed request")?;
    assert!(matches!(response, RpcResponse::MailTail { .. }));
    Ok(())
}

async fn assert_runtime_route(path: &std::path::Path) -> Result<()> {
    let payload = RuntimeClient::new(path).version().await?;
    assert_eq!(payload.version.protocol_version, RUNTIME_PROTOCOL_VERSION);
    Ok(())
}

async fn shutdown_composed_host(
    endpoint: &DaemonEndpoint,
    compose_task: JoinHandle<Result<()>>,
) -> Result<()> {
    let response = send_request(endpoint, &SessionRpc::Shutdown).await?;
    assert!(matches!(response, RpcResponse::Shutdown { .. }));
    tokio::time::timeout(REQUEST_TIMEOUT, compose_task)
        .await
        .context("composed host exits within two seconds")???;
    Ok(())
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
        context_id: Some(THREAD.to_owned()),
        selector: None,
        recipient: None,
        include_system: false,
    }
}
