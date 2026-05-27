mod common;
use common::OrPanic as _;

use common::{
    LOCAL_UID, TestDaemon, local_context, mail_count, spawn_test_session,
    spawn_test_session_with_labels,
};
use lilo_im_core::{Action, AuditDecision, Principal};
use lilo_session_core::{
    DeleteRequest, IsolationPolicy, Label, MailReadRequest, MailSendRequest, NudgeRequest,
    RpcRequest, RpcResponse, RuntimeKind, Selector, SpawnRequest,
};
use lilo_session_daemon::handler::DaemonState;
use lilo_session_daemon::identity_client::RequestContext;
use uuid::Uuid;

#[tokio::test]
async fn mail_round_trip_marks_read() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;

    let sent = daemon
        .state
        .handle(
            context.clone(),
            RpcRequest::MailSend {
                request: MailSendRequest {
                    from: Some(sender.id.to_string()),
                    to: Selector::Id { id: recipient.id },
                    content: "review the spec".to_string(),
                },
            },
        )
        .await;
    assert!(matches!(sent.response, RpcResponse::MailSent { .. }));
    assert_eq!(
        mail_count(&daemon.state, context.clone(), recipient.id).await,
        1
    );

    let read = daemon
        .state
        .handle(
            context.clone(),
            RpcRequest::MailRead {
                request: MailReadRequest {
                    selector: Selector::Id { id: recipient.id },
                    peek: false,
                },
            },
        )
        .await;
    let RpcResponse::MailRead { response } = read.response else {
        panic!("expected mail read response");
    };
    assert_eq!(response.mail.len(), 1);
    assert_eq!(response.mail[0].content, "review the spec");
    assert_eq!(mail_count(&daemon.state, context, recipient.id).await, 0);
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
            context.clone(),
            RpcRequest::MailSend {
                request: MailSendRequest {
                    from: Some(sender.id.to_string()),
                    to: Selector::Label {
                        key: "area".to_string(),
                        op: lilo_session_core::LabelOp::Eq {
                            value: "auth".to_string(),
                        },
                    },
                    content: "merge by Friday".to_string(),
                },
            },
        )
        .await;
    let RpcResponse::MailSent { response } = sent.response else {
        panic!("expected mail sent response");
    };
    assert_eq!(response.mail.len(), 2);
    assert_eq!(
        response
            .mail
            .iter()
            .map(|mail| mail.recipient_id)
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
            RpcRequest::Nudge {
                request: NudgeRequest {
                    to: Selector::Role {
                        name: "engineer".to_string(),
                    },
                    content: "review PRs".to_string(),
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
            RpcRequest::MailSend {
                request: MailSendRequest {
                    from: None,
                    to: Selector::Id { id: Uuid::now_v7() },
                    content: "review the spec".to_string(),
                },
            },
        )
        .await;

    let RpcResponse::Error { message } = sent.response else {
        panic!("expected error response");
    };
    assert!(message.contains("unknown recipient session"));
}

#[tokio::test]
async fn mail_send_skips_terminated_recipients() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let live = spawn_test_session(&daemon, &context, "engineer").await;
    let dead = spawn_test_session(&daemon, &context, "engineer").await;
    let _ = daemon
        .state
        .handle(
            context.clone(),
            RpcRequest::Delete {
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
            RpcRequest::MailSend {
                request: MailSendRequest {
                    from: None,
                    to: Selector::All,
                    content: "broadcast".to_string(),
                },
            },
        )
        .await;
    let RpcResponse::MailSent { response } = sent.response else {
        panic!("expected mail sent");
    };
    let delivered: Vec<_> = response.mail.iter().map(|m| m.recipient_id).collect();
    assert_eq!(delivered, vec![live.id]);
    let skipped: Vec<_> = response.errors.iter().map(|e| e.target.as_str()).collect();
    assert_eq!(skipped, vec![dead.id.to_string().as_str()]);
    assert!(response.errors[0].message.contains("TERMINATED"));
}

#[tokio::test]
async fn nudge_delegates_delivery_outcome_from_driver() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;
    let nudged = daemon
        .state
        .handle(
            context,
            RpcRequest::Nudge {
                request: NudgeRequest {
                    to: Selector::Id { id: recipient.id },
                    content: "ping".to_string(),
                },
            },
        )
        .await;

    let RpcResponse::Nudged { response } = nudged.response else {
        panic!("expected nudge response");
    };
    assert_eq!(response.nudges.len(), 1);
    assert!(response.nudges[0].delivered);
    assert_eq!(response.nudges[0].message, "delivered");
}

#[tokio::test]
async fn nudge_surfaces_failed_outcome_from_driver() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    daemon.driver.set_nudge(lilo_session_driver::NudgeResult {
        delivered: false,
        message: "tmux pane is no longer available".to_string(),
    });
    let context = local_context();
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;
    let nudged = daemon
        .state
        .handle(
            context,
            RpcRequest::Nudge {
                request: NudgeRequest {
                    to: Selector::Id { id: recipient.id },
                    content: "ping".to_string(),
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
        "tmux pane is no longer available"
    );
}

#[tokio::test]
async fn successful_mutations_write_allow_audit_rows() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let recipient = spawn_test_session(&daemon, &context, "engineer").await;

    send_read_nudge_delete(&daemon.state, context, sender.id, recipient.id).await;

    let rows =
        lilo_im_store::query_audit(&daemon.audit_path, lilo_im_store::AuditFilters::default())
            .await
            .or_panic("audit query succeeds");
    let actions = rows.iter().map(|row| row.action).collect::<Vec<_>>();
    assert_eq!(
        actions,
        vec![
            Action::Spawn,
            Action::Spawn,
            Action::MailSend,
            Action::MailRead,
            Action::Nudge,
            Action::Kill,
        ]
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
            RpcRequest::Spawn {
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

    let rows =
        lilo_im_store::query_audit(&daemon.audit_path, lilo_im_store::AuditFilters::default())
            .await
            .or_panic("audit query succeeds");
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
        .lock()
        .or_panic("store lock poisoned")
        .list_sessions(None)
        .or_panic("session list succeeds");
    assert!(sessions.is_empty());
}

async fn send_read_nudge_delete(
    state: &DaemonState,
    context: RequestContext,
    sender_id: Uuid,
    recipient_id: Uuid,
) {
    let requests = [
        RpcRequest::MailSend {
            request: MailSendRequest {
                from: Some(sender_id.to_string()),
                to: Selector::Id { id: recipient_id },
                content: "review the spec".to_string(),
            },
        },
        RpcRequest::MailRead {
            request: MailReadRequest {
                selector: Selector::Id { id: recipient_id },
                peek: false,
            },
        },
        RpcRequest::Nudge {
            request: NudgeRequest {
                to: Selector::Id { id: recipient_id },
                content: "ping".to_string(),
            },
        },
        RpcRequest::Delete {
            request: DeleteRequest {
                selector: Selector::Id { id: recipient_id },
                signal: "SIGTERM".to_string(),
                grace_secs: 5,
            },
        },
    ];

    for request in requests {
        let response = state.handle(context.clone(), request).await.response;
        assert!(!matches!(response, RpcResponse::Error { .. }));
    }
}
