use chrono::{DateTime, Utc};
use lilo_session_core::Mail;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
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
    #[error("{field} out of range: {value}")]
    IntegerOutOfRange { field: &'static str, value: i64 },
}

impl SqliteStore {
    pub async fn insert_mail(&self, mail: &Mail) -> Result<(), MailRowError> {
        sqlx::query(
            "INSERT INTO session_mail (id, sender_id, recipient_id, content, sent_at, read_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(mail.id.to_string())
        .bind(mail.sender_id.to_string())
        .bind(mail.recipient_id.to_string())
        .bind(&mail.content)
        .bind(mail.sent_at.to_rfc3339())
        .bind(mail.read_at.map(|timestamp| timestamp.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn count_unread_mail(&self, recipient_id: &Uuid) -> Result<usize, MailRowError> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM session_mail WHERE recipient_id = ? AND read_at IS NULL",
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
        let mail = self.list_unread_mail(recipient_id).await?;
        if !peek && !mail.is_empty() {
            let mut tx = self.pool.begin().await?;
            for item in &mail {
                sqlx::query("UPDATE session_mail SET read_at = ? WHERE id = ? AND read_at IS NULL")
                    .bind(read_at.to_rfc3339())
                    .bind(item.id.to_string())
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
        }
        Ok(mail)
    }

    async fn list_unread_mail(&self, recipient_id: &Uuid) -> Result<Vec<Mail>, MailRowError> {
        self.query_mail(
            "SELECT * FROM session_mail
             WHERE recipient_id = ? AND read_at IS NULL
             ORDER BY sent_at",
            [recipient_id.to_string()],
        )
        .await
    }

    async fn query_mail<const N: usize>(
        &self,
        sql: &str,
        params: [String; N],
    ) -> Result<Vec<Mail>, MailRowError> {
        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(param);
        }
        let rows = query.fetch_all(&self.pool).await?;
        rows.iter().map(mail_from_row).collect()
    }
}

fn mail_from_row(row: &SqliteRow) -> Result<Mail, MailRowError> {
    Ok(Mail {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)?,
        sender_id: Uuid::parse_str(&row.try_get::<String, _>("sender_id")?)?,
        recipient_id: Uuid::parse_str(&row.try_get::<String, _>("recipient_id")?)?,
        content: row.try_get("content")?,
        sent_at: parse_timestamp(&row.try_get::<String, _>("sent_at")?)?,
        read_at: parse_optional_timestamp(row.try_get::<Option<String>, _>("read_at")?)?,
    })
}

fn integer_out_of_range(field: &'static str, value: i64) -> MailRowError {
    MailRowError::IntegerOutOfRange { field, value }
}

#[cfg(test)]
mod tests {
    use crate::test_support::OrPanic as _;
    use chrono::Utc;

    use super::*;

    #[tokio::test]
    async fn mail_round_trip_marks_read() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let now = Utc::now();
        let mail = Mail {
            id: Uuid::now_v7(),
            sender_id: Uuid::now_v7(),
            recipient_id: Uuid::now_v7(),
            content: "review the spec".to_string(),
            sent_at: now,
            read_at: None,
        };

        store.insert_mail(&mail).await.or_panic("mail inserts");

        assert_eq!(
            store
                .count_unread_mail(&mail.recipient_id)
                .await
                .or_panic("unread count"),
            1
        );
        assert_eq!(
            store
                .read_unread_mail(&mail.recipient_id, Utc::now(), false)
                .await
                .or_panic("mail reads"),
            vec![mail.clone()]
        );
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
        let mail = Mail {
            id: Uuid::now_v7(),
            sender_id: Uuid::now_v7(),
            recipient_id: Uuid::now_v7(),
            content: "review the spec".to_string(),
            sent_at: Utc::now(),
            read_at: None,
        };

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
    async fn unread_count_stays_fast_on_populated_mail_table() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let recipient_id = Uuid::now_v7();
        for index in 0..1_000 {
            store
                .insert_mail(&Mail {
                    id: Uuid::now_v7(),
                    sender_id: Uuid::now_v7(),
                    recipient_id,
                    content: format!("message {index}"),
                    sent_at: Utc::now(),
                    read_at: None,
                })
                .await
                .or_panic("mail inserts");
        }

        let started = std::time::Instant::now();
        let unread = store
            .count_unread_mail(&recipient_id)
            .await
            .or_panic("unread count");

        assert_eq!(unread, 1_000);
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
    }
}
