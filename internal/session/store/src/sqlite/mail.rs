use std::collections::BTreeSet;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use lilo_session_core::{Mail, MailIntent, SenderRef, SmError};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;
use uuid::Uuid;

use super::SqliteStore;
use super::time::{parse_optional_timestamp, parse_timestamp};

#[derive(Debug, Error)]
pub enum MailRowError {
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
    #[error(transparent)]
    Chrono(#[from] chrono::ParseError),
    #[error(transparent)]
    Uuid(#[from] uuid::Error),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error(transparent)]
    Session(#[from] SmError),
    #[error("{field} out of range: {value}")]
    IntegerOutOfRange { field: &'static str, value: i64 },
    #[error("idempotency key {key} conflicts with existing mail")]
    IdempotencyConflict { key: String },
}

impl SqliteStore {
    pub async fn insert_mail(&self, mail: &Mail) -> Result<Mail, MailRowError> {
        let mut inserted = self
            .insert_mail_for_recipients(mail, &[mail.recipient_id])
            .await?;
        Ok(inserted.remove(0))
    }

    pub async fn insert_mail_for_recipients(
        &self,
        mail: &Mail,
        recipient_ids: &[Uuid],
    ) -> Result<Vec<Mail>, MailRowError> {
        if recipient_ids.is_empty() {
            return Ok(Vec::new());
        }
        let sender_ref = serde_json::to_string(&mail.sender)?;
        let mut transaction = self.pool.begin().await?;
        let mail = if let Some(key) = &mail.idempotency_key {
            if let Some(existing) =
                message_by_idempotency(&mut transaction, &sender_ref, key).await?
            {
                validate_idempotent_replay(&mut transaction, &existing, mail, recipient_ids)
                    .await?;
                load_message_deliveries(&mut transaction, &existing, recipient_ids).await?
            } else {
                insert_message(&mut transaction, mail, &sender_ref).await?;
                insert_deliveries(&mut transaction, mail, recipient_ids).await?;
                mail_for_recipients(mail, recipient_ids)
            }
        } else {
            insert_message(&mut transaction, mail, &sender_ref).await?;
            insert_deliveries(&mut transaction, mail, recipient_ids).await?;
            mail_for_recipients(mail, recipient_ids)
        };
        transaction.commit().await?;
        Ok(mail)
    }

    pub async fn count_unread_mail(&self, recipient_id: &Uuid) -> Result<usize, MailRowError> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM message_deliveries
             WHERE recipient_session_id = ?
               AND status = 'unread'",
        )
        .bind(recipient_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        usize::try_from(count).map_err(|_| integer_out_of_range("unread_count", count))
    }

    pub async fn read_unread_mail(
        &self,
        recipient_id: &Uuid,
        read_at: DateTime<Utc>,
        peek: bool,
    ) -> Result<Vec<Mail>, MailRowError> {
        if peek {
            return self.list_unread_mail(recipient_id).await;
        }

        let mut transaction = self.pool.begin().await?;
        let mut mail = list_unread_mail_in(&mut transaction, recipient_id).await?;
        for item in &mail {
            sqlx::query(
                "UPDATE message_deliveries
                 SET status = 'read', read_at = ?
                 WHERE message_id = ?
                   AND recipient_session_id = ?
                   AND status = 'unread'",
            )
            .bind(read_at.to_rfc3339())
            .bind(item.id.to_string())
            .bind(recipient_id.to_string())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        for item in &mut mail {
            item.read_at = Some(read_at);
        }
        Ok(mail)
    }

    pub async fn count_conversation_depth(&self, context_id: &str) -> Result<usize, MailRowError> {
        count_query(
            self.pool(),
            "SELECT COUNT(*)
             FROM messages
             WHERE context_id = ?
               AND intent != 'receipt'",
            [context_id.to_string()],
            "conversation_depth",
        )
        .await
    }

    pub async fn count_sender_rate_since(
        &self,
        sender: &SenderRef,
        since: DateTime<Utc>,
    ) -> Result<usize, MailRowError> {
        count_query(
            self.pool(),
            "SELECT COUNT(*)
             FROM messages
             WHERE sender_ref = ?
               AND intent != 'receipt'
               AND sent_at >= ?",
            [serde_json::to_string(sender)?, since.to_rfc3339()],
            "sender_rate",
        )
        .await
    }

    async fn list_unread_mail(&self, recipient_id: &Uuid) -> Result<Vec<Mail>, MailRowError> {
        let rows = sqlx::query(UNREAD_MAIL_SQL)
            .bind(recipient_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(mail_from_row).collect()
    }
}

async fn insert_message(
    transaction: &mut Transaction<'_, Sqlite>,
    mail: &Mail,
    sender_ref: &str,
) -> Result<(), MailRowError> {
    sqlx::query(
        "INSERT INTO messages
         (message_id, sender_ref, context_id, intent, idempotency_key, content, sent_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(mail.id.to_string())
    .bind(sender_ref)
    .bind(&mail.context_id)
    .bind(mail.intent.to_string())
    .bind(&mail.idempotency_key)
    .bind(&mail.content)
    .bind(mail.sent_at.to_rfc3339())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_deliveries(
    transaction: &mut Transaction<'_, Sqlite>,
    mail: &Mail,
    recipient_ids: &[Uuid],
) -> Result<(), MailRowError> {
    for recipient_id in recipient_ids {
        sqlx::query(
            "INSERT INTO message_deliveries
             (message_id, recipient_session_id, status, read_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(mail.id.to_string())
        .bind(recipient_id.to_string())
        .bind(mail.status().to_string())
        .bind(mail.read_at.map(|timestamp| timestamp.to_rfc3339()))
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

fn mail_from_row(row: &SqliteRow) -> Result<Mail, MailRowError> {
    Ok(Mail {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)?,
        sender: serde_json::from_str::<SenderRef>(&row.try_get::<String, _>("sender_ref")?)?,
        recipient_id: Uuid::parse_str(&row.try_get::<String, _>("recipient_id")?)?,
        content: row.try_get("content")?,
        sent_at: parse_timestamp(&row.try_get::<String, _>("sent_at")?)?,
        read_at: parse_optional_timestamp(row.try_get::<Option<String>, _>("read_at")?)?,
        context_id: row.try_get("context_id")?,
        intent: MailIntent::from_str(&row.try_get::<String, _>("intent")?)?,
        idempotency_key: row.try_get("idempotency_key")?,
    })
}

#[derive(Clone)]
struct StoredMessage {
    id: Uuid,
    sender: SenderRef,
    content: String,
    sent_at: DateTime<Utc>,
    context_id: String,
    intent: MailIntent,
    idempotency_key: Option<String>,
}

impl StoredMessage {
    fn to_mail(&self, recipient_id: Uuid, read_at: Option<DateTime<Utc>>) -> Mail {
        Mail {
            id: self.id,
            sender: self.sender.clone(),
            recipient_id,
            content: self.content.clone(),
            sent_at: self.sent_at,
            read_at,
            context_id: self.context_id.clone(),
            intent: self.intent,
            idempotency_key: self.idempotency_key.clone(),
        }
    }
}

async fn message_by_idempotency(
    transaction: &mut Transaction<'_, Sqlite>,
    sender_ref: &str,
    key: &str,
) -> Result<Option<StoredMessage>, MailRowError> {
    sqlx::query(
        "SELECT message_id AS id, sender_ref, content, sent_at, context_id, intent, idempotency_key
         FROM messages
         WHERE sender_ref = ?
           AND idempotency_key = ?",
    )
    .bind(sender_ref)
    .bind(key)
    .fetch_optional(&mut **transaction)
    .await?
    .map(|row| stored_message_from_row(&row))
    .transpose()
}

async fn validate_idempotent_replay(
    transaction: &mut Transaction<'_, Sqlite>,
    existing: &StoredMessage,
    mail: &Mail,
    recipient_ids: &[Uuid],
) -> Result<(), MailRowError> {
    let key = mail.idempotency_key.clone().unwrap_or_default();
    let matches_message = existing.sender == mail.sender
        && existing.content == mail.content
        && existing.context_id == mail.context_id
        && existing.intent == mail.intent
        && existing.idempotency_key == mail.idempotency_key;
    if !matches_message {
        return Err(MailRowError::IdempotencyConflict { key });
    }
    let existing_recipients = recipient_set_for_message(transaction, &existing.id).await?;
    if existing_recipients != recipient_set(recipient_ids) {
        return Err(MailRowError::IdempotencyConflict { key });
    }
    Ok(())
}

async fn recipient_set_for_message(
    transaction: &mut Transaction<'_, Sqlite>,
    message_id: &Uuid,
) -> Result<BTreeSet<String>, MailRowError> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT recipient_session_id
         FROM message_deliveries
         WHERE message_id = ?",
    )
    .bind(message_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    Ok(rows.into_iter().collect())
}

fn recipient_set(recipient_ids: &[Uuid]) -> BTreeSet<String> {
    recipient_ids.iter().map(Uuid::to_string).collect()
}

async fn load_message_deliveries(
    transaction: &mut Transaction<'_, Sqlite>,
    message: &StoredMessage,
    recipient_ids: &[Uuid],
) -> Result<Vec<Mail>, MailRowError> {
    let mut mail = Vec::new();
    for recipient_id in recipient_ids {
        let read_at = sqlx::query_scalar::<_, Option<String>>(
            "SELECT read_at
             FROM message_deliveries
             WHERE message_id = ?
               AND recipient_session_id = ?",
        )
        .bind(message.id.to_string())
        .bind(recipient_id.to_string())
        .fetch_one(&mut **transaction)
        .await?;
        mail.push(message.to_mail(*recipient_id, parse_optional_timestamp(read_at)?));
    }
    Ok(mail)
}

fn mail_for_recipients(mail: &Mail, recipient_ids: &[Uuid]) -> Vec<Mail> {
    recipient_ids
        .iter()
        .map(|recipient_id| Mail {
            recipient_id: *recipient_id,
            ..mail.clone()
        })
        .collect()
}

async fn list_unread_mail_in(
    transaction: &mut Transaction<'_, Sqlite>,
    recipient_id: &Uuid,
) -> Result<Vec<Mail>, MailRowError> {
    let rows = sqlx::query(UNREAD_MAIL_SQL)
        .bind(recipient_id.to_string())
        .fetch_all(&mut **transaction)
        .await?;
    rows.iter().map(mail_from_row).collect()
}

const UNREAD_MAIL_SQL: &str = "
    SELECT m.message_id AS id,
           m.sender_ref,
           d.recipient_session_id AS recipient_id,
           m.content,
           m.sent_at,
           d.read_at,
           m.context_id,
           m.intent,
           m.idempotency_key
    FROM message_deliveries d
    JOIN messages m ON m.message_id = d.message_id
    WHERE d.recipient_session_id = ?
      AND d.status = 'unread'
    ORDER BY m.sent_at, m.message_id
";

fn stored_message_from_row(row: &SqliteRow) -> Result<StoredMessage, MailRowError> {
    Ok(StoredMessage {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)?,
        sender: serde_json::from_str::<SenderRef>(&row.try_get::<String, _>("sender_ref")?)?,
        content: row.try_get("content")?,
        sent_at: parse_timestamp(&row.try_get::<String, _>("sent_at")?)?,
        context_id: row.try_get("context_id")?,
        intent: MailIntent::from_str(&row.try_get::<String, _>("intent")?)?,
        idempotency_key: row.try_get("idempotency_key")?,
    })
}

async fn count_query<const N: usize>(
    pool: &sqlx::SqlitePool,
    sql: &str,
    params: [String; N],
    field: &'static str,
) -> Result<usize, MailRowError> {
    let mut query = sqlx::query_scalar::<_, i64>(sql);
    for param in params {
        query = query.bind(param);
    }
    let count = query.fetch_one(pool).await?;
    usize::try_from(count).map_err(|_| integer_out_of_range(field, count))
}

fn integer_out_of_range(field: &'static str, value: i64) -> MailRowError {
    MailRowError::IntegerOutOfRange { field, value }
}

#[cfg(test)]
mod tests {
    use crate::test_support::OrPanic as _;
    use chrono::{Duration, Utc};

    use super::*;

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
}
