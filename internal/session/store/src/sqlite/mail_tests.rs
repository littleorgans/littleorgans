use chrono::{Duration, Utc};
use lilo_common::id::{MessageId, SessionId};
use lilo_session_core::{LostEvidence, Mail, MailIntent, MailStatus, SenderRef};

use super::{MailRowError, SqliteStore};
use crate::test_support::OrPanic as _;

#[tokio::test]
async fn mail_round_trip_marks_read() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let mail = test_mail(
        SenderRef::session(SessionId::from_uuid(uuid::Uuid::now_v7())),
        SessionId::from_uuid(uuid::Uuid::now_v7()),
        "review the spec",
        "review-thread",
        None,
    );

    store.insert_mail(&mail).await.or_panic("mail inserts");

    assert_eq!(
        store
            .count_unread_mail(&mail.recipient_id)
            .await
            .or_panic("unread count"),
        1
    );
    let read = store
        .read_unread_mail(&mail.recipient_id, Utc::now(), false)
        .await
        .or_panic("mail reads");
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].id, mail.id);
    assert!(read[0].read_at.is_some());
    assert_eq!(
        store
            .count_unread_mail(&mail.recipient_id)
            .await
            .or_panic("unread count"),
        0
    );
}

#[tokio::test]
async fn peek_keeps_mail_unread() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let mail = test_mail(
        SenderRef::session(SessionId::from_uuid(uuid::Uuid::now_v7())),
        SessionId::from_uuid(uuid::Uuid::now_v7()),
        "review the spec",
        "review-thread",
        None,
    );

    store.insert_mail(&mail).await.or_panic("mail inserts");
    let read = store
        .read_unread_mail(&mail.recipient_id, Utc::now(), true)
        .await
        .or_panic("mail peeks");

    assert_eq!(read, vec![mail.clone()]);
    assert_eq!(
        store
            .count_unread_mail(&mail.recipient_id)
            .await
            .or_panic("unread count"),
        1
    );
}

#[tokio::test]
async fn idempotent_retry_collapses_to_existing_message() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let sender = SenderRef::session(SessionId::from_uuid(uuid::Uuid::now_v7()));
    let recipient_ids = [
        SessionId::from_uuid(uuid::Uuid::now_v7()),
        SessionId::from_uuid(uuid::Uuid::now_v7()),
    ];
    let mail = test_mail(
        sender.clone(),
        recipient_ids[0],
        "send once",
        "idempotent-thread",
        Some("send-1"),
    );

    store
        .insert_mail_for_recipients(&mail, &recipient_ids)
        .await
        .or_panic("first send inserts");
    let replay = Mail {
        id: MessageId::from_uuid(uuid::Uuid::now_v7()),
        sender,
        recipient_id: recipient_ids[0],
        sent_at: Utc::now() + Duration::seconds(1),
        ..mail.clone()
    };
    let collapsed = store
        .insert_mail_for_recipients(&replay, &recipient_ids)
        .await
        .or_panic("retry collapses");

    assert_eq!(
        collapsed.iter().map(|item| item.id).collect::<Vec<_>>(),
        vec![mail.id, mail.id]
    );
    assert_eq!(message_count(&store).await, 1);
    assert_eq!(delivery_count(&store).await, 2);
}

#[tokio::test]
async fn concurrent_idempotent_sends_collapse_to_one_message() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let sender = SenderRef::session(SessionId::from_uuid(uuid::Uuid::now_v7()));
    let recipient_ids = [
        SessionId::from_uuid(uuid::Uuid::now_v7()),
        SessionId::from_uuid(uuid::Uuid::now_v7()),
    ];
    let first = test_mail(
        sender.clone(),
        recipient_ids[0],
        "send once",
        "idempotent-thread",
        Some("send-race"),
    );
    let second = Mail {
        id: MessageId::from_uuid(uuid::Uuid::now_v7()),
        sent_at: Utc::now() + Duration::milliseconds(1),
        ..first.clone()
    };
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let first_store = store.clone();
    let second_store = store.clone();
    let first_barrier = std::sync::Arc::clone(&barrier);
    let second_barrier = std::sync::Arc::clone(&barrier);

    let first_task = tokio::spawn(async move {
        first_barrier.wait().await;
        first_store
            .insert_mail_for_recipients_with_outcome(&first, &recipient_ids)
            .await
    });
    let second_task = tokio::spawn(async move {
        second_barrier.wait().await;
        second_store
            .insert_mail_for_recipients_with_outcome(&second, &recipient_ids)
            .await
    });

    let first_outcome = first_task.await.or_panic("first task joins");
    let second_outcome = second_task.await.or_panic("second task joins");
    let outcomes = [
        first_outcome.or_panic("first send succeeds"),
        second_outcome.or_panic("second send succeeds"),
    ];

    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.inserted).count(),
        1
    );
    assert_eq!(
        outcomes.iter().filter(|outcome| !outcome.inserted).count(),
        1
    );
    assert_eq!(message_count(&store).await, 1);
    assert_eq!(delivery_count(&store).await, 2);
}

#[tokio::test]
async fn idempotent_retry_with_different_recipients_conflicts() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let sender = SenderRef::session(SessionId::from_uuid(uuid::Uuid::now_v7()));
    let first_recipient = SessionId::from_uuid(uuid::Uuid::now_v7());
    let second_recipient = SessionId::from_uuid(uuid::Uuid::now_v7());
    let mail = test_mail(
        sender.clone(),
        first_recipient,
        "send once",
        "idempotent-thread",
        Some("send-1"),
    );

    store
        .insert_mail_for_recipients(&mail, &[first_recipient])
        .await
        .or_panic("first send inserts");
    let replay = Mail {
        id: MessageId::from_uuid(uuid::Uuid::now_v7()),
        sender,
        recipient_id: second_recipient,
        ..mail
    };

    let error = store
        .insert_mail_for_recipients(&replay, &[second_recipient])
        .await
        .expect_err("recipient change conflicts");
    assert!(matches!(error, MailRowError::IdempotencyConflict { .. }));
    assert_eq!(message_count(&store).await, 1);
}

#[tokio::test]
async fn unread_reads_order_by_sent_at_then_message_id() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let recipient_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let sender = SenderRef::session(SessionId::from_uuid(uuid::Uuid::now_v7()));
    let sent_at = Utc::now();
    let first = Mail {
        id: MessageId::from_uuid(uuid::Uuid::from_u128(1)),
        sent_at,
        ..test_mail(sender.clone(), recipient_id, "first", "order-thread", None)
    };
    let second = Mail {
        id: MessageId::from_uuid(uuid::Uuid::from_u128(2)),
        sent_at,
        ..test_mail(sender, recipient_id, "second", "order-thread", None)
    };

    store.insert_mail(&second).await.or_panic("second inserts");
    store.insert_mail(&first).await.or_panic("first inserts");
    let read = store
        .read_unread_mail(&recipient_id, Utc::now(), true)
        .await
        .or_panic("mail peeks");

    assert_eq!(
        read.iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );
}

#[tokio::test]
async fn read_side_decode_error_does_not_mark_read() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let message_id = MessageId::from_uuid(uuid::Uuid::now_v7());
    let recipient_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    sqlx::query(
        "INSERT INTO messages
         (message_id, sender_ref, context_id, intent, idempotency_key, content, sent_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(message_id.to_string())
    .bind("not-json")
    .bind("broken-thread")
    .bind("request")
    .bind(Option::<String>::None)
    .bind("broken")
    .bind(Utc::now().to_rfc3339())
    .execute(store.pool())
    .await
    .or_panic("message inserts");
    sqlx::query(
        "INSERT INTO message_deliveries
         (message_id, recipient_session_id, status, read_at)
         VALUES (?, ?, 'unread', NULL)",
    )
    .bind(message_id.to_string())
    .bind(recipient_id.to_string())
    .execute(store.pool())
    .await
    .or_panic("delivery inserts");

    store
        .read_unread_mail(&recipient_id, Utc::now(), false)
        .await
        .expect_err("decode error is returned");

    assert_eq!(
        store
            .count_unread_mail(&recipient_id)
            .await
            .or_panic("unread count"),
        1
    );
}

#[tokio::test]
async fn breaker_counts_exclude_receipts() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let sender = SenderRef::session(SessionId::from_uuid(uuid::Uuid::now_v7()));
    let recipient_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let since = Utc::now() - Duration::seconds(1);
    let request = test_mail(
        sender.clone(),
        recipient_id,
        "request",
        "breaker-thread",
        None,
    );
    let receipt = Mail {
        id: MessageId::from_uuid(uuid::Uuid::now_v7()),
        intent: MailIntent::Receipt,
        ..test_mail(
            sender.clone(),
            recipient_id,
            "receipt",
            "breaker-thread",
            None,
        )
    };

    store
        .insert_mail(&request)
        .await
        .or_panic("request inserts");
    store
        .insert_mail(&receipt)
        .await
        .or_panic("receipt inserts");

    assert_eq!(
        store
            .count_conversation_depth("breaker-thread")
            .await
            .or_panic("depth counts"),
        1
    );
    assert_eq!(
        store
            .count_sender_rate_since(&sender, since)
            .await
            .or_panic("rate counts"),
        1
    );
}

#[tokio::test]
async fn terminating_recipient_marks_unread_mail_undeliverable() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let recipient = SessionId::from_uuid(uuid::Uuid::now_v7());
    let other = SessionId::from_uuid(uuid::Uuid::now_v7());

    let pending = test_mail(
        SenderRef::session(SessionId::from_uuid(uuid::Uuid::now_v7())),
        recipient,
        "what are we working on?",
        "testing",
        None,
    );
    store
        .insert_mail(&pending)
        .await
        .or_panic("pending inserts");
    let live = test_mail(
        SenderRef::session(SessionId::from_uuid(uuid::Uuid::now_v7())),
        other,
        "still here",
        "testing",
        None,
    );
    store.insert_mail(&live).await.or_panic("live inserts");

    store
        .mark_session_terminated(&recipient, Some(0), Utc::now())
        .await
        .or_panic("recipient terminates");

    // The orphaned mail is no longer counted as unread...
    assert_eq!(
        store
            .count_unread_mail(&recipient)
            .await
            .or_panic("unread count"),
        0
    );
    // ...but stays visible in the transcript as undeliverable.
    let log = store
        .list_message_log(Some("testing"), None, Some(&[recipient]), true, None)
        .await
        .or_panic("message log");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].status, MailStatus::Undeliverable);

    // A live recipient's mail is untouched.
    assert_eq!(
        store
            .count_unread_mail(&other)
            .await
            .or_panic("other unread count"),
        1
    );
}

#[tokio::test]
async fn terminating_recipient_leaves_read_mail_read() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let recipient = SessionId::from_uuid(uuid::Uuid::now_v7());
    let mail = test_mail(
        SenderRef::session(SessionId::from_uuid(uuid::Uuid::now_v7())),
        recipient,
        "seen it",
        "testing",
        None,
    );
    store.insert_mail(&mail).await.or_panic("mail inserts");
    store
        .read_unread_mail(&recipient, Utc::now(), false)
        .await
        .or_panic("mail reads");

    store
        .mark_session_terminated(&recipient, Some(0), Utc::now())
        .await
        .or_panic("recipient terminates");

    let log = store
        .list_message_log(Some("testing"), None, Some(&[recipient]), true, None)
        .await
        .or_panic("message log");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].status, MailStatus::Read);
}

#[tokio::test]
async fn losing_recipient_marks_unread_mail_undeliverable() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let recipient = SessionId::from_uuid(uuid::Uuid::now_v7());
    let mail = test_mail(
        SenderRef::session(SessionId::from_uuid(uuid::Uuid::now_v7())),
        recipient,
        "are you there?",
        "testing",
        None,
    );
    store.insert_mail(&mail).await.or_panic("mail inserts");

    store
        .mark_session_lost(&recipient, LostEvidence::PidNotAlive, Utc::now())
        .await
        .or_panic("recipient lost");

    assert_eq!(
        store
            .count_unread_mail(&recipient)
            .await
            .or_panic("unread count"),
        0
    );
    let log = store
        .list_message_log(Some("testing"), None, Some(&[recipient]), true, None)
        .await
        .or_panic("message log");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].status, MailStatus::Undeliverable);
}

fn test_mail(
    sender: SenderRef,
    recipient_id: SessionId,
    content: &str,
    context_id: &str,
    idempotency_key: Option<&str>,
) -> Mail {
    Mail {
        id: MessageId::from_uuid(uuid::Uuid::now_v7()),
        sender,
        recipient_id,
        content: content.to_string(),
        sent_at: Utc::now(),
        read_at: None,
        status: MailStatus::Unread,
        context_id: context_id.to_string(),
        intent: MailIntent::Request,
        idempotency_key: idempotency_key.map(ToString::to_string),
    }
}

async fn message_count(store: &SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(store.pool())
        .await
        .or_panic("message count")
}

async fn delivery_count(store: &SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM message_deliveries")
        .fetch_one(store.pool())
        .await
        .or_panic("delivery count")
}
