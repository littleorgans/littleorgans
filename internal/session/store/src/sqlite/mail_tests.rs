use chrono::{Duration, Utc};
use lilo_session_core::{Mail, MailIntent, SenderRef};
use uuid::Uuid;

use super::{MailRowError, SqliteStore};
use crate::test_support::OrPanic as _;

#[tokio::test]
async fn mail_round_trip_marks_read() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let mail = test_mail(
        SenderRef::session(Uuid::now_v7()),
        Uuid::now_v7(),
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
        SenderRef::session(Uuid::now_v7()),
        Uuid::now_v7(),
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
    let sender = SenderRef::session(Uuid::now_v7());
    let recipient_ids = [Uuid::now_v7(), Uuid::now_v7()];
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
        id: Uuid::now_v7(),
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
async fn idempotent_retry_with_different_recipients_conflicts() {
    let (_dir, store) = SqliteStore::open_temp().await;
    let sender = SenderRef::session(Uuid::now_v7());
    let first_recipient = Uuid::now_v7();
    let second_recipient = Uuid::now_v7();
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
        id: Uuid::now_v7(),
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
    let recipient_id = Uuid::now_v7();
    let sender = SenderRef::session(Uuid::now_v7());
    let sent_at = Utc::now();
    let first = Mail {
        id: Uuid::from_u128(1),
        sent_at,
        ..test_mail(sender.clone(), recipient_id, "first", "order-thread", None)
    };
    let second = Mail {
        id: Uuid::from_u128(2),
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
    let message_id = Uuid::now_v7();
    let recipient_id = Uuid::now_v7();
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
    let sender = SenderRef::session(Uuid::now_v7());
    let recipient_id = Uuid::now_v7();
    let since = Utc::now() - Duration::seconds(1);
    let request = test_mail(
        sender.clone(),
        recipient_id,
        "request",
        "breaker-thread",
        None,
    );
    let receipt = Mail {
        id: Uuid::now_v7(),
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

fn test_mail(
    sender: SenderRef,
    recipient_id: Uuid,
    content: &str,
    context_id: &str,
    idempotency_key: Option<&str>,
) -> Mail {
    Mail {
        id: Uuid::now_v7(),
        sender,
        recipient_id,
        content: content.to_string(),
        sent_at: Utc::now(),
        read_at: None,
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
