mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::OrPanic as _;
use common::{
    LOCAL_UID, RecordingIdentityPort, TestDaemon, local_context, mail_count, mail_request,
    spawn_test_session,
};
use lilo_common::id::{MessageId, SessionId};
use lilo_im_core::Action;
use lilo_rm_core::{EventBatch, EventsRequest, Lifecycle, NudgeMode, StatusFilter};
use lilo_session_core::{
    MailDeliveryStatus, MailIntent, MailLogFilter, MailNotifyMode, MailNotifyStatus,
    MailPeekRequest, MailReadRequest, MailSendRequest, MailSendResponse, MessageView, RpcResponse,
    RuntimeDoctorReport, Selector, SenderView, SessionRpc,
};
use lilo_session_daemon::handler::DaemonState;
use lilo_session_daemon::identity_client::IdentityPort;
use lilo_session_driver::{
    CaptureResult, ChildExit, NudgeResult, RuntimeError, RuntimePort, SpawnLaunch, SpawnedProcess,
};
use tokio::time::timeout;

type TestRuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + Send + 'a>>;

#[tokio::test]
async fn depth_breaker_trips_one_context_only() {
    let mut daemon = TestDaemon::new(LOCAL_UID).await;
    daemon.state.set_mail_safety_limits(2, 100, 60);
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;

    for content in ["first", "second"] {
        send_mail(
            &daemon.state,
            context.clone().with_mcp_caller_session_id(sender.id),
            mail_request(
                Selector::Id { id: recipient.id },
                content,
                "depth-thread",
                MailIntent::Request,
            ),
        )
        .await;
    }

    let rejected = send_mail_response(
        &daemon.state,
        context.clone().with_mcp_caller_session_id(sender.id),
        mail_request(
            Selector::Id { id: recipient.id },
            "third",
            "depth-thread",
            MailIntent::Request,
        ),
    )
    .await;
    let RpcResponse::Error { message } = rejected else {
        panic!("expected depth breaker error");
    };
    assert!(message.contains("conversation depth"), "{message}");
    assert_eq!(
        mail_count(&daemon.state, context.clone(), recipient.id).await,
        2
    );

    send_mail(
        &daemon.state,
        context.clone().with_mcp_caller_session_id(sender.id),
        mail_request(
            Selector::Id { id: recipient.id },
            "other context",
            "other-thread",
            MailIntent::Request,
        ),
    )
    .await;
    assert_eq!(mail_count(&daemon.state, context, recipient.id).await, 3);
}

#[tokio::test]
async fn rate_breaker_throttles_sender_across_contexts() {
    let mut daemon = TestDaemon::new(LOCAL_UID).await;
    daemon.state.set_mail_safety_limits(100, 2, 60);
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let other_sender = spawn_test_session(&daemon, &context, "reviewer").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;

    for (content, context_id) in [("first", "rate-one"), ("second", "rate-two")] {
        send_mail(
            &daemon.state,
            context.clone().with_mcp_caller_session_id(sender.id),
            mail_request(
                Selector::Id { id: recipient.id },
                content,
                context_id,
                MailIntent::Request,
            ),
        )
        .await;
    }

    let rejected = send_mail_response(
        &daemon.state,
        context.clone().with_mcp_caller_session_id(sender.id),
        mail_request(
            Selector::Id { id: recipient.id },
            "third",
            "rate-three",
            MailIntent::Request,
        ),
    )
    .await;
    let RpcResponse::Error { message } = rejected else {
        panic!("expected rate breaker error");
    };
    assert!(message.contains("throttled sender"));
    assert_eq!(
        mail_count(&daemon.state, context.clone(), recipient.id).await,
        2
    );

    send_mail(
        &daemon.state,
        context.clone().with_mcp_caller_session_id(other_sender.id),
        mail_request(
            Selector::Id { id: recipient.id },
            "other sender",
            "rate-four",
            MailIntent::Request,
        ),
    )
    .await;
    assert_eq!(mail_count(&daemon.state, context, recipient.id).await, 3);
}

#[tokio::test]
async fn idempotent_retry_returns_original_after_breaker_saturation() {
    let mut daemon = TestDaemon::new(LOCAL_UID).await;
    daemon.state.set_mail_safety_limits(1, 100, 60);
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;
    let mut request = mail_request(
        Selector::Id { id: recipient.id },
        "send once",
        "idempotent-breaker",
        MailIntent::Request,
    );
    request.idempotency_key = Some("send-1".to_string());

    let first = send_mail(
        &daemon.state,
        context.clone().with_mcp_caller_session_id(sender.id),
        request.clone(),
    )
    .await;
    let first_id = message_id(&first);
    let replay = send_mail(
        &daemon.state,
        context.clone().with_mcp_caller_session_id(sender.id),
        request,
    )
    .await;

    assert_eq!(message_id(&replay), first_id);
    assert_eq!(
        mail_count(&daemon.state, context.clone(), recipient.id).await,
        1
    );

    let rejected = send_mail_response(
        &daemon.state,
        context.with_mcp_caller_session_id(sender.id),
        mail_request(
            Selector::Id { id: recipient.id },
            "new send",
            "idempotent-breaker",
            MailIntent::Request,
        ),
    )
    .await;
    assert!(matches!(rejected, RpcResponse::Error { .. }));
}

#[tokio::test]
async fn notify_nudge_authz_failure_preserves_mail() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;
    let identity = Arc::new(RecordingIdentityPort::with_decisions([
        Ok(()),
        Err("nudge denied by test identity".to_string()),
    ]));
    let state = daemon.state_with_identity_port(Arc::clone(&identity) as Arc<dyn IdentityPort>);
    let mut request = mail_request(
        Selector::Id { id: recipient.id },
        "wake and review",
        "notify-thread",
        MailIntent::Request,
    );
    request.notify = Some(MailNotifyMode::Steer);

    let response = send_mail(
        &state,
        context.clone().with_mcp_caller_session_id(sender.id),
        request,
    )
    .await;

    assert_eq!(response.results[0].mail, MailDeliveryStatus::Ok);
    assert_eq!(response.results[0].notify, MailNotifyStatus::Err);
    assert_eq!(
        response.results[0].error.as_deref(),
        Some("nudge denied by test identity")
    );
    assert_eq!(mail_count(&state, context, recipient.id).await, 1);
    assert_eq!(
        identity
            .calls()
            .iter()
            .take(2)
            .map(|call| call.action)
            .collect::<Vec<_>>(),
        vec![Action::MailSend, Action::Nudge]
    );
}

#[tokio::test]
async fn notify_runtime_failure_is_warning_not_mail_failure() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;
    let runtime = Arc::new(RecordingRuntimePort::failing_nudge("runtime offline"));
    let state = daemon
        .state_with_runtime_port(Arc::clone(&runtime) as Arc<dyn RuntimePort>)
        .await;
    let mut request = mail_request(
        Selector::Id { id: recipient.id },
        "wake and review",
        "runtime-notify-thread",
        MailIntent::Request,
    );
    request.notify = Some(MailNotifyMode::Wait);

    let response = send_mail(
        &state,
        context.clone().with_mcp_caller_session_id(sender.id),
        request,
    )
    .await;

    assert_eq!(response.results[0].mail, MailDeliveryStatus::Ok);
    assert_eq!(response.results[0].notify, MailNotifyStatus::Err);
    assert!(
        response.results[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("runtime offline")
    );
    assert_eq!(mail_count(&state, context, recipient.id).await, 1);
    assert_eq!(
        runtime.nudges(),
        vec![(recipient.id.to_string(), "you have mail".to_string(), None)]
    );
}

#[tokio::test]
async fn notify_wait_timeout_is_forwarded_to_runtime_port() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;
    let runtime = Arc::new(RecordingRuntimePort::new());
    let state = daemon
        .state_with_runtime_port(Arc::clone(&runtime) as Arc<dyn RuntimePort>)
        .await;
    let mut request = mail_request(
        Selector::Id { id: recipient.id },
        "wake and review",
        "runtime-notify-timeout-thread",
        MailIntent::Request,
    );
    request.notify = Some(MailNotifyMode::Wait);
    request.timeout_ms = Some(2_000);

    let response = send_mail(
        &state,
        context.with_mcp_caller_session_id(sender.id),
        request,
    )
    .await;

    assert_eq!(response.results[0].mail, MailDeliveryStatus::Ok);
    assert_eq!(response.results[0].notify, MailNotifyStatus::Ok);
    assert_eq!(
        runtime.nudges(),
        vec![(
            recipient.id.to_string(),
            "you have mail".to_string(),
            Some(2_000)
        )]
    );
}

#[tokio::test]
async fn notify_timeout_requires_wait_on_handler_path() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;
    let mut request = mail_request(
        Selector::Id { id: recipient.id },
        "wake and review",
        "runtime-notify-timeout-reject-thread",
        MailIntent::Request,
    );
    request.notify = Some(MailNotifyMode::Steer);
    request.timeout_ms = Some(2_000);

    let response = send_mail_response(
        &daemon.state,
        context.clone().with_mcp_caller_session_id(sender.id),
        request,
    )
    .await;

    let RpcResponse::Error { message } = response else {
        panic!("expected validation error");
    };
    assert!(message.contains("requires --notify wait"), "{message}");
    assert_eq!(mail_count(&daemon.state, context, recipient.id).await, 0);
}

#[tokio::test]
async fn mail_append_event_fires_once_per_persisted_message() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let _first = spawn_test_session(&daemon, &context, "engineer").await;
    let _second = spawn_test_session(&daemon, &context, "engineer").await;
    let mut events = daemon.state.subscribe_mail_appends();

    let response = send_mail(
        &daemon.state,
        context.with_mcp_caller_session_id(sender.id),
        mail_request(
            Selector::Role {
                name: "engineer".to_string(),
            },
            "fanout",
            "append-thread",
            MailIntent::Inform,
        ),
    )
    .await;

    let event = timeout(Duration::from_secs(1), events.recv())
        .await
        .or_panic("append event arrives")
        .or_panic("append event decodes");
    assert_eq!(event.message_id, message_id(&response));
    assert!(
        timeout(Duration::from_millis(50), events.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn read_receipts_emit_on_drain_only_for_session_senders() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;
    let request = mail_request(
        Selector::Id { id: recipient.id },
        "needs ack",
        "receipt-thread",
        MailIntent::Request,
    );
    let sent = send_mail(
        &daemon.state,
        context.clone().with_mcp_caller_session_id(sender.id),
        request,
    )
    .await;

    let peeked = peek_mail(&daemon.state, context.clone(), recipient.id).await;
    assert_eq!(peeked.messages.len(), 1);
    assert_eq!(
        mail_count(&daemon.state, context.clone(), sender.id).await,
        0
    );

    let read = read_mail(
        &daemon.state,
        context.clone().with_mcp_caller_session_id(recipient.id),
    )
    .await;
    assert_eq!(message_id_from_view(&read.messages[0]), message_id(&sent));
    assert_eq!(
        mail_count(&daemon.state, context.clone(), sender.id).await,
        1
    );

    let receipts = read_mail(
        &daemon.state,
        context.clone().with_mcp_caller_session_id(sender.id),
    )
    .await;
    assert_eq!(receipts.messages.len(), 1);
    assert_eq!(receipts.messages[0].sender, SenderView::System);
    assert_eq!(receipts.messages[0].intent, MailIntent::Receipt);
    assert!(
        receipts.messages[0]
            .content
            .contains(&recipient.id.to_string())
    );
    assert_eq!(
        mail_count(&daemon.state, context.clone(), sender.id).await,
        0
    );

    send_mail(
        &daemon.state,
        context.clone(),
        mail_request(
            Selector::Id { id: recipient.id },
            "operator origin",
            "operator-receipt-thread",
            MailIntent::Inform,
        ),
    )
    .await;
    let operator_message = read_mail(
        &daemon.state,
        context.clone().with_mcp_caller_session_id(recipient.id),
    )
    .await;
    assert_eq!(operator_message.messages.len(), 1);
    assert_eq!(mail_count(&daemon.state, context, sender.id).await, 0);
}

async fn send_mail(
    state: &DaemonState,
    context: lilo_session_daemon::identity_client::RequestContext,
    request: MailSendRequest,
) -> MailSendResponse {
    let response = send_mail_response(state, context, request).await;
    let RpcResponse::MailSent { response } = response else {
        panic!("expected mail sent response");
    };
    response
}

async fn send_mail_response(
    state: &DaemonState,
    context: lilo_session_daemon::identity_client::RequestContext,
    request: MailSendRequest,
) -> RpcResponse {
    state
        .handle(context, SessionRpc::MailSend { request })
        .await
        .response
}

async fn read_mail(
    state: &DaemonState,
    context: lilo_session_daemon::identity_client::RequestContext,
) -> lilo_session_core::MailReadResponse {
    let read = state
        .handle(
            context,
            SessionRpc::MailRead {
                request: MailReadRequest {},
            },
        )
        .await;
    let RpcResponse::MailRead { response } = read.response else {
        panic!("expected mail read response");
    };
    response
}

async fn peek_mail(
    state: &DaemonState,
    context: lilo_session_daemon::identity_client::RequestContext,
    recipient_id: SessionId,
) -> lilo_session_core::MailPeekResponse {
    let peek = state
        .handle(
            context,
            SessionRpc::MailPeek {
                request: MailPeekRequest {
                    filter: MailLogFilter {
                        context_id: None,
                        selector: None,
                        recipient: Some(Selector::Id { id: recipient_id }),
                        include_system: false,
                    },
                },
            },
        )
        .await;
    let RpcResponse::MailPeek { response } = peek.response else {
        panic!("expected mail peek response");
    };
    response
}

fn message_id(response: &MailSendResponse) -> MessageId {
    message_id_from_view(
        response.results[0]
            .message
            .as_ref()
            .or_panic("mail send includes message"),
    )
}

fn message_id_from_view(message: &MessageView) -> MessageId {
    message.id
}

#[derive(Default)]
struct RecordingRuntimePort {
    nudges: Mutex<Vec<(String, String, Option<u64>)>>,
    nudge_error: Option<String>,
}

impl RecordingRuntimePort {
    fn new() -> Self {
        Self {
            nudges: Mutex::new(Vec::new()),
            nudge_error: None,
        }
    }

    fn failing_nudge(message: &str) -> Self {
        Self {
            nudges: Mutex::new(Vec::new()),
            nudge_error: Some(message.to_string()),
        }
    }

    fn nudges(&self) -> Vec<(String, String, Option<u64>)> {
        self.nudges.lock().or_panic("nudge lock").clone()
    }
}

impl RuntimePort for RecordingRuntimePort {
    fn spawn<'a>(
        &'a self,
        _session_id: &'a str,
        _launch: &'a SpawnLaunch,
    ) -> TestRuntimeFuture<'a, SpawnedProcess> {
        unsupported("spawn")
    }

    fn reap_exited(&self) -> TestRuntimeFuture<'_, Vec<ChildExit>> {
        unsupported("reap_exited")
    }

    fn capture<'a>(
        &'a self,
        _session_id: &'a str,
        _scrollback_lines: Option<u32>,
    ) -> TestRuntimeFuture<'a, CaptureResult> {
        unsupported("capture")
    }

    fn terminate<'a>(
        &'a self,
        _session_id: &'a str,
        _signal: &'a str,
        _grace: Duration,
    ) -> TestRuntimeFuture<'a, Option<ChildExit>> {
        unsupported("terminate")
    }

    fn nudge<'a>(
        &'a self,
        session_id: &'a str,
        content: &'a str,
        _mode: NudgeMode,
        timeout_ms: Option<u64>,
    ) -> TestRuntimeFuture<'a, NudgeResult> {
        Box::pin(async move {
            self.nudges.lock().or_panic("nudge lock").push((
                session_id.to_string(),
                content.to_string(),
                timeout_ms,
            ));
            match &self.nudge_error {
                Some(message) => Err(RuntimeError::local(message)),
                None => Ok(NudgeResult {
                    delivered: true,
                    message: "delivered".to_string(),
                }),
            }
        })
    }

    fn status(&self, _filter: StatusFilter) -> TestRuntimeFuture<'_, Vec<Lifecycle>> {
        unsupported("status")
    }

    fn poll_events(&self, _request: EventsRequest) -> TestRuntimeFuture<'_, EventBatch> {
        unsupported("poll_events")
    }

    fn doctor(&self) -> TestRuntimeFuture<'_, RuntimeDoctorReport> {
        unsupported("doctor")
    }

    fn terminate_all(&self) {}
}

fn unsupported<T: Send + 'static>(operation: &'static str) -> TestRuntimeFuture<'static, T> {
    Box::pin(async move {
        Err(RuntimeError::local(format!(
            "unsupported test runtime operation {operation}"
        )))
    })
}
