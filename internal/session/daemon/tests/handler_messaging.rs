mod common;
use common::OrPanic as _;

use std::sync::Arc;

use common::shared_test_support::assert_ordered_subsequence;
use common::{
    LOCAL_UID, RecordingIdentityPort, TestDaemon, local_context, mail_count, mail_request,
    spawn_test_session, spawn_test_session_with_labels,
};
use lilo_common::id::SessionId;
use lilo_im_core::{Action, AuditDecision, Principal};
use lilo_rm_core::NudgeMode;
use lilo_session_core::{
    DeleteRequest, IsolationPolicy, Label, MailDeliveryStatus, MailIntent, MailReadRequest,
    NudgeRequest, RpcResponse, RuntimeKind, Selector, SenderView, SessionRpc, SessionState,
    SpawnRequest,
};
use lilo_session_daemon::handler::DaemonState;
use lilo_session_daemon::identity_client::{IdentityPort, RequestContext};

#[tokio::test]
async fn mail_round_trip_marks_read() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;

    let sent = daemon
        .state
        .handle(
            context.clone().with_mcp_caller_session_id(sender.id),
            SessionRpc::MailSend {
                request: mail_request(
                    Selector::Id { id: recipient.id },
                    "review the spec",
                    "review-thread",
                    MailIntent::Request,
                ),
            },
        )
        .await;
    let RpcResponse::MailSent { response } = sent.response else {
        panic!("expected mail sent response");
    };
    let message = response.results[0]
        .message
        .as_ref()
        .or_panic("send result includes message");
    assert_eq!(
        message.sender,
        SenderView::Session {
            session_id: sender.id,
            role: "pm".to_string(),
            display_label: "pm".to_string(),
            labels: Vec::new(),
            namespace: sender.namespace.clone(),
        }
    );
    assert_eq!(
        mail_count(&daemon.state, context.clone(), recipient.id).await,
        1
    );

    let read = daemon
        .state
        .handle(
            context.clone().with_mcp_caller_session_id(recipient.id),
            SessionRpc::MailRead {
                request: MailReadRequest {},
            },
        )
        .await;
    let RpcResponse::MailRead { response } = read.response else {
        panic!("expected mail read response");
    };
    assert_eq!(response.messages.len(), 1);
    assert_eq!(response.messages[0].content, "review the spec");
    assert_eq!(mail_count(&daemon.state, context, recipient.id).await, 0);
}

#[tokio::test]
async fn operator_send_uses_operator_sender_view() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;

    let sent = daemon
        .state
        .handle(
            context,
            SessionRpc::MailSend {
                request: mail_request(
                    Selector::Id { id: recipient.id },
                    "review the spec",
                    "review-thread",
                    MailIntent::Request,
                ),
            },
        )
        .await;
    let RpcResponse::MailSent { response } = sent.response else {
        panic!("expected mail sent response");
    };
    let message = response.results[0]
        .message
        .as_ref()
        .or_panic("send result includes message");
    let SenderView::Operator {
        principal,
        display_label,
    } = &message.sender
    else {
        panic!("expected operator sender");
    };
    assert_eq!(*display_label, "operator");
    assert_eq!(
        *principal,
        serde_json::to_value(Principal::Local(LOCAL_UID)).or_panic("principal serializes")
    );
}

#[tokio::test]
async fn mail_read_drains_only_caller_mailbox() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let first = spawn_test_session(&daemon, &context, "engineer").await;
    let second = spawn_test_session(&daemon, &context, "reviewer").await;

    for (recipient, content) in [(first.id, "first"), (second.id, "second")] {
        let sent = daemon
            .state
            .handle(
                context.clone().with_mcp_caller_session_id(sender.id),
                SessionRpc::MailSend {
                    request: mail_request(
                        Selector::Id { id: recipient },
                        content,
                        "review-thread",
                        MailIntent::Request,
                    ),
                },
            )
            .await;
        assert!(matches!(sent.response, RpcResponse::MailSent { .. }));
    }

    let read = daemon
        .state
        .handle(
            context.clone().with_mcp_caller_session_id(first.id),
            SessionRpc::MailRead {
                request: MailReadRequest {},
            },
        )
        .await;
    let RpcResponse::MailRead { response } = read.response else {
        panic!("expected mail read response");
    };
    assert_eq!(response.messages.len(), 1);
    assert_eq!(response.messages[0].recipient.session_id, first.id);
    assert_eq!(response.messages[0].content, "first");
    assert_eq!(
        mail_count(&daemon.state, context.clone(), first.id).await,
        0
    );
    assert_eq!(mail_count(&daemon.state, context, second.id).await, 1);
}

#[tokio::test]
async fn selector_mail_and_nudge_fan_out_to_matching_sessions() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let auth_one = spawn_test_session_with_labels(
        &daemon,
        &context,
        "engineer",
        vec![Label {
            key: "area".to_string(),
            value: "auth".to_string(),
        }],
    )
    .await;
    let auth_two = spawn_test_session_with_labels(
        &daemon,
        &context,
        "engineer",
        vec![Label {
            key: "area".to_string(),
            value: "auth".to_string(),
        }],
    )
    .await;
    let ui = spawn_test_session_with_labels(
        &daemon,
        &context,
        "engineer",
        vec![Label {
            key: "area".to_string(),
            value: "ui".to_string(),
        }],
    )
    .await;

    let sent = daemon
        .state
        .handle(
            context.clone().with_mcp_caller_session_id(sender.id),
            SessionRpc::MailSend {
                request: mail_request(
                    Selector::Label {
                        key: "area".to_string(),
                        op: lilo_session_core::LabelOp::Eq {
                            value: "auth".to_string(),
                        },
                    },
                    "merge by Friday",
                    "merge-thread",
                    MailIntent::Inform,
                ),
            },
        )
        .await;
    let RpcResponse::MailSent { response } = sent.response else {
        panic!("expected mail sent response");
    };
    assert_eq!(response.results.len(), 2);
    assert_eq!(
        response
            .results
            .iter()
            .filter_map(|result| result.message.as_ref())
            .map(|message| message.recipient.session_id)
            .collect::<Vec<_>>(),
        vec![auth_one.id, auth_two.id]
    );
    assert_eq!(
        mail_count(&daemon.state, context.clone(), auth_one.id).await,
        1
    );
    assert_eq!(
        mail_count(&daemon.state, context.clone(), auth_two.id).await,
        1
    );
    assert_eq!(mail_count(&daemon.state, context.clone(), ui.id).await, 0);

    let nudged = daemon
        .state
        .handle(
            context,
            SessionRpc::Nudge {
                request: NudgeRequest {
                    to: Selector::Role {
                        name: "engineer".to_string(),
                    },
                    content: "review PRs".to_string(),
                    mode: NudgeMode::Immediate,
                },
            },
        )
        .await;
    let RpcResponse::Nudged { response } = nudged.response else {
        panic!("expected nudge response");
    };
    assert_eq!(response.nudges.len(), 3);
}

#[tokio::test]
async fn mail_send_rejects_unknown_recipient() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let sent = daemon
        .state
        .handle(
            local_context(),
            SessionRpc::MailSend {
                request: mail_request(
                    Selector::Id {
                        id: SessionId::from_uuid(uuid::Uuid::now_v7()),
                    },
                    "review the spec",
                    "review-thread",
                    MailIntent::Request,
                ),
            },
        )
        .await;

    let RpcResponse::Error { message } = sent.response else {
        panic!("expected error response");
    };
    assert!(message.contains("unknown recipient session"));
}

#[tokio::test]
async fn mail_send_rejects_client_receipt_intent() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;
    let sent = daemon
        .state
        .handle(
            context,
            SessionRpc::MailSend {
                request: mail_request(
                    Selector::Id { id: recipient.id },
                    "read receipt",
                    "review-thread",
                    MailIntent::Receipt,
                ),
            },
        )
        .await;

    let RpcResponse::Error { message } = sent.response else {
        panic!("expected error response");
    };
    assert!(message.contains("receipt is reserved"));
}

#[tokio::test]
async fn mail_send_uses_injected_identity_port() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;
    let identity = Arc::new(RecordingIdentityPort::denying(
        "mail denied by test identity",
    ));
    let state = daemon.state_with_identity_port(Arc::clone(&identity) as Arc<dyn IdentityPort>);

    let sent = state
        .handle(
            context.with_mcp_caller_session_id(sender.id),
            SessionRpc::MailSend {
                request: mail_request(
                    Selector::Id { id: recipient.id },
                    "review the spec",
                    "review-thread",
                    MailIntent::Request,
                ),
            },
        )
        .await;

    let RpcResponse::MailSent { response } = sent.response else {
        panic!("expected mail sent response");
    };
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].mail, MailDeliveryStatus::Err);
    assert_eq!(
        response.results[0].error.as_deref(),
        Some("mail denied by test identity")
    );
    assert_eq!(
        state
            .store
            .count_unread_mail(&recipient.id)
            .await
            .or_panic("unread mail count succeeds"),
        0
    );

    let calls = identity.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].principal, Principal::Local(LOCAL_UID));
    assert_eq!(calls[0].action, Action::MailSend);
    assert_eq!(calls[0].resource.session_id, Some(recipient.id));
}

#[tokio::test]
async fn mail_send_targets_only_running_recipients() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let live = spawn_test_session(&daemon, &context, "engineer").await;
    let dead = spawn_test_session(&daemon, &context, "engineer").await;
    let mut spawning = live.clone();
    spawning.id = SessionId::from_uuid(uuid::Uuid::now_v7());
    spawning.state = SessionState::Spawning;
    daemon
        .state
        .store
        .insert_session(&spawning)
        .await
        .or_panic("spawning session inserts");
    let _ = daemon
        .state
        .handle(
            context.clone(),
            SessionRpc::Delete {
                request: DeleteRequest {
                    selector: Selector::Id { id: dead.id },
                    signal: "SIGTERM".to_string(),
                    grace_secs: 5,
                },
            },
        )
        .await;

    let sent = daemon
        .state
        .handle(
            context,
            SessionRpc::MailSend {
                request: mail_request(
                    Selector::All,
                    "broadcast",
                    "broadcast-thread",
                    MailIntent::Inform,
                ),
            },
        )
        .await;
    let RpcResponse::MailSent { response } = sent.response else {
        panic!("expected mail sent");
    };
    let delivered: Vec<_> = response
        .results
        .iter()
        .filter_map(|result| result.message.as_ref())
        .map(|message| message.recipient.session_id)
        .collect();
    assert_eq!(delivered, vec![live.id]);
    assert_eq!(response.results.len(), 1);
    assert!(response.errors.is_empty());
    assert_eq!(mail_count(&daemon.state, local_context(), live.id).await, 1);
    assert_eq!(mail_count(&daemon.state, local_context(), dead.id).await, 0);
    assert_eq!(
        mail_count(&daemon.state, local_context(), spawning.id).await,
        0
    );
}

#[tokio::test]
async fn nudge_reports_runtime_headless_outcome() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;
    let nudged = daemon
        .state
        .handle(
            context,
            SessionRpc::Nudge {
                request: NudgeRequest {
                    to: Selector::Id { id: recipient.id },
                    content: "ping".to_string(),
                    mode: NudgeMode::Immediate,
                },
            },
        )
        .await;

    let RpcResponse::Nudged { response } = nudged.response else {
        panic!("expected nudge response");
    };
    assert_eq!(response.nudges.len(), 1);
    assert!(!response.nudges[0].delivered);
    assert_eq!(
        response.nudges[0].message,
        "headless runtime does not support nudges"
    );
}

#[tokio::test]
async fn successful_mutations_write_allow_audit_rows() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;

    send_read_nudge_delete(&daemon.state, context, sender.id, recipient.id).await;

    let rows = daemon.audit_rows().await;
    let actions = rows.iter().map(|row| row.action).collect::<Vec<_>>();
    assert_ordered_subsequence(
        &actions,
        &[
            Action::Spawn,
            Action::Spawn,
            Action::Spawn,
            Action::Spawn,
            Action::MailSend,
            Action::MailRead,
            Action::Nudge,
            Action::Nudge,
            Action::Kill,
            Action::Kill,
        ],
    );
    assert!(rows.iter().all(|row| row.decision == AuditDecision::Allow));
}

#[tokio::test]
async fn denied_mutation_is_audited_without_mutating_store() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let denied_context = RequestContext::new(Principal::Local(LOCAL_UID + 1));
    let response = daemon
        .state
        .handle(
            denied_context,
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "general".to_string(),
                    workspace: daemon.dir.path().display().to_string(),
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

    let RpcResponse::Error { message } = response.response else {
        panic!("expected authz error response");
    };
    assert!(message.contains("unknown principal"));

    let rows = daemon.audit_rows().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].action, Action::Spawn);
    assert_eq!(
        rows[0].decision,
        AuditDecision::Deny {
            reason: "non-local uid".to_string(),
        }
    );
    let sessions = daemon
        .state
        .store
        .list_sessions(None)
        .await
        .or_panic("session list succeeds");
    assert!(sessions.is_empty());
}

async fn send_read_nudge_delete(
    state: &DaemonState,
    context: RequestContext,
    sender_id: SessionId,
    recipient_id: SessionId,
) {
    let context = context.with_mcp_caller_session_id(sender_id);
    let response = state
        .handle(
            context.clone(),
            SessionRpc::MailSend {
                request: mail_request(
                    Selector::Id { id: recipient_id },
                    "review the spec",
                    "review-thread",
                    MailIntent::Request,
                ),
            },
        )
        .await
        .response;
    assert!(!matches!(response, RpcResponse::Error { .. }));

    let response = state
        .handle(
            context.with_mcp_caller_session_id(recipient_id),
            SessionRpc::MailRead {
                request: MailReadRequest {},
            },
        )
        .await
        .response;
    assert!(!matches!(response, RpcResponse::Error { .. }));

    for request in [
        SessionRpc::Nudge {
            request: NudgeRequest {
                to: Selector::Id { id: recipient_id },
                content: "ping".to_string(),
                mode: NudgeMode::Immediate,
            },
        },
        SessionRpc::Delete {
            request: DeleteRequest {
                selector: Selector::Id { id: recipient_id },
                signal: "SIGTERM".to_string(),
                grace_secs: 5,
            },
        },
    ] {
        let response = state.handle(local_context(), request).await.response;
        assert!(!matches!(response, RpcResponse::Error { .. }));
    }
}
