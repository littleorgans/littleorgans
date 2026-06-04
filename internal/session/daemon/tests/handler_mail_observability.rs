mod common;

use common::{LOCAL_UID, TestDaemon, local_context, mail_count, mail_request};
use lilo_common::id::SessionId;
use lilo_session_core::{
    MailIntent, MailLogFilter, MailPeekRequest, MailReadRequest, MailTailRequest, RpcResponse,
    Selector, SenderView, SessionRpc,
};

#[tokio::test]
async fn operator_peek_and_tail_do_not_drain_mail() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = common::spawn_test_session(&daemon, &context, "pm").await;
    let recipient = common::spawn_test_session(&daemon, &context, "engineer").await;

    send_mail(&daemon, sender.id, recipient.id).await;

    let peeked = peek_context(&daemon, "observability-thread", false).await;
    assert_eq!(peeked.len(), 1);
    assert_eq!(peeked[0].content, "observe me");
    assert_eq!(
        mail_count(&daemon.state, context.clone(), recipient.id).await,
        1
    );

    let tailed = tail_context(&daemon, "observability-thread").await;
    assert_eq!(tailed.len(), 1);
    assert_eq!(tailed[0].recipient.session_id, recipient.id);
    assert_eq!(
        mail_count(&daemon.state, context.clone(), recipient.id).await,
        1
    );

    read_mail(&daemon, recipient.id).await;
    assert_eq!(
        mail_count(&daemon.state, context.clone(), recipient.id).await,
        0
    );
    assert_eq!(mail_count(&daemon.state, context, sender.id).await, 1);
}

#[tokio::test]
async fn operator_transcript_hides_system_receipts_by_default() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = common::spawn_test_session(&daemon, &context, "pm").await;
    let recipient = common::spawn_test_session(&daemon, &context, "engineer").await;

    send_mail(&daemon, sender.id, recipient.id).await;
    read_mail(&daemon, recipient.id).await;

    let default_transcript = peek_context(&daemon, "observability-thread", false).await;
    assert_eq!(default_transcript.len(), 1);
    assert_ne!(default_transcript[0].sender, SenderView::System);

    let system_transcript = peek_context(&daemon, "observability-thread", true).await;
    assert_eq!(system_transcript.len(), 2);
    assert!(
        system_transcript
            .iter()
            .any(|message| message.sender == SenderView::System)
    );
    assert_eq!(mail_count(&daemon.state, context, sender.id).await, 1);
}

async fn send_mail(daemon: &TestDaemon, sender_id: SessionId, recipient_id: SessionId) {
    let sent = daemon
        .state
        .handle(
            local_context().with_mcp_caller_session_id(sender_id),
            SessionRpc::MailSend {
                request: mail_request(
                    Selector::Id { id: recipient_id },
                    "observe me",
                    "observability-thread",
                    MailIntent::Request,
                ),
            },
        )
        .await;
    assert!(matches!(sent.response, RpcResponse::MailSent { .. }));
}

async fn read_mail(daemon: &TestDaemon, recipient_id: SessionId) {
    let read = daemon
        .state
        .handle(
            local_context().with_mcp_caller_session_id(recipient_id),
            SessionRpc::MailRead {
                request: MailReadRequest {},
            },
        )
        .await;
    assert!(matches!(read.response, RpcResponse::MailRead { .. }));
}

async fn peek_context(
    daemon: &TestDaemon,
    context_id: &str,
    include_system: bool,
) -> Vec<lilo_session_core::MessageView> {
    let peek = daemon
        .state
        .handle(
            local_context(),
            SessionRpc::MailPeek {
                request: MailPeekRequest {
                    filter: log_filter(context_id, include_system),
                },
            },
        )
        .await;
    let RpcResponse::MailPeek { response } = peek.response else {
        panic!("expected mail peek response");
    };
    response.messages
}

async fn tail_context(
    daemon: &TestDaemon,
    context_id: &str,
) -> Vec<lilo_session_core::MessageView> {
    let tail = daemon
        .state
        .handle(
            local_context(),
            SessionRpc::MailTail {
                request: MailTailRequest {
                    filter: log_filter(context_id, false),
                    after: None,
                    follow: false,
                    wait_ms: None,
                },
            },
        )
        .await;
    let RpcResponse::MailTail { response } = tail.response else {
        panic!("expected mail tail response");
    };
    response.messages
}

fn log_filter(context_id: &str, include_system: bool) -> MailLogFilter {
    MailLogFilter {
        context_id: Some(context_id.to_string()),
        selector: None,
        recipient: None,
        include_system,
    }
}
