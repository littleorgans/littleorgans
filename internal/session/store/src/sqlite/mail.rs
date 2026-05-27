use chrono::{DateTime, Utc};
use lilo_session_core::Mail;
use rusqlite::{Row, params, params_from_iter};
use thiserror::Error;
use uuid::Uuid;

use super::SqliteStore;
use super::time::{parse_optional_timestamp, parse_timestamp};

#[derive(Debug, Error)]
pub enum MailRowError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Chrono(#[from] chrono::ParseError),
    #[error(transparent)]
    Uuid(#[from] uuid::Error),
    #[error("{field} out of range: {value}")]
    IntegerOutOfRange { field: &'static str, value: i64 },
}

impl SqliteStore {
    pub fn insert_mail(&self, mail: &Mail) -> Result<(), MailRowError> {
        self.connection.execute(
            "INSERT INTO mail (id, sender_id, recipient_id, content, sent_at, read_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                mail.id.to_string(),
                mail.sender_id.to_string(),
                mail.recipient_id.to_string(),
                &mail.content,
                mail.sent_at.to_rfc3339(),
                mail.read_at.map(|timestamp| timestamp.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn count_unread_mail(&self, recipient_id: &Uuid) -> Result<usize, MailRowError> {
        let count = self.connection.query_row(
            "SELECT COUNT(*) FROM mail WHERE recipient_id = ?1 AND read_at IS NULL",
            [recipient_id.to_string()],
            |row| row.get::<_, i64>(0),
        )?;
        usize::try_from(count).map_err(|_| integer_out_of_range("unread_count", count))
    }

    pub fn read_unread_mail(
        &mut self,
        recipient_id: &Uuid,
        read_at: DateTime<Utc>,
        peek: bool,
    ) -> Result<Vec<Mail>, MailRowError> {
        let mail = self.list_unread_mail(recipient_id)?;
        if !peek && !mail.is_empty() {
            let tx = self.connection.transaction()?;
            for item in &mail {
                tx.execute(
                    "UPDATE mail SET read_at = ?1 WHERE id = ?2 AND read_at IS NULL",
                    params![read_at.to_rfc3339(), item.id.to_string()],
                )?;
            }
            tx.commit()?;
        }
        Ok(mail)
    }

    fn list_unread_mail(&self, recipient_id: &Uuid) -> Result<Vec<Mail>, MailRowError> {
        self.query_mail(
            "SELECT * FROM mail
             WHERE recipient_id = ?1 AND read_at IS NULL
             ORDER BY sent_at",
            [recipient_id.to_string()],
        )
    }

    fn query_mail<const N: usize>(
        &self,
        sql: &str,
        params: [String; N],
    ) -> Result<Vec<Mail>, MailRowError> {
        let mut statement = self.connection.prepare(sql)?;
        let mut rows = statement.query(params_from_iter(params))?;
        let mut mail = Vec::new();
        while let Some(row) = rows.next()? {
            mail.push(mail_from_row(row)?);
        }
        Ok(mail)
    }
}

fn mail_from_row(row: &Row<'_>) -> Result<Mail, MailRowError> {
    Ok(Mail {
        id: Uuid::parse_str(&row.get::<_, String>("id")?)?,
        sender_id: Uuid::parse_str(&row.get::<_, String>("sender_id")?)?,
        recipient_id: Uuid::parse_str(&row.get::<_, String>("recipient_id")?)?,
        content: row.get("content")?,
        sent_at: parse_timestamp(&row.get::<_, String>("sent_at")?)?,
        read_at: parse_optional_timestamp(row.get::<_, Option<String>>("read_at")?)?,
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

    #[test]
    fn mail_round_trip_marks_read() {
        let mut store = SqliteStore::open_in_memory().or_panic("store opens");
        let now = Utc::now();
        let mail = Mail {
            id: Uuid::now_v7(),
            sender_id: Uuid::now_v7(),
            recipient_id: Uuid::now_v7(),
            content: "review the spec".to_string(),
            sent_at: now,
            read_at: None,
        };

        store.insert_mail(&mail).or_panic("mail inserts");

        assert_eq!(
            store
                .count_unread_mail(&mail.recipient_id)
                .or_panic("unread count"),
            1
        );
        assert_eq!(
            store
                .read_unread_mail(&mail.recipient_id, Utc::now(), false)
                .or_panic("mail reads"),
            vec![mail.clone()]
        );
        assert_eq!(
            store
                .count_unread_mail(&mail.recipient_id)
                .or_panic("unread count"),
            0
        );
    }

    #[test]
    fn peek_keeps_mail_unread() {
        let mut store = SqliteStore::open_in_memory().or_panic("store opens");
        let mail = Mail {
            id: Uuid::now_v7(),
            sender_id: Uuid::now_v7(),
            recipient_id: Uuid::now_v7(),
            content: "review the spec".to_string(),
            sent_at: Utc::now(),
            read_at: None,
        };

        store.insert_mail(&mail).or_panic("mail inserts");
        let read = store
            .read_unread_mail(&mail.recipient_id, Utc::now(), true)
            .or_panic("mail peeks");

        assert_eq!(read, vec![mail.clone()]);
        assert_eq!(
            store
                .count_unread_mail(&mail.recipient_id)
                .or_panic("unread count"),
            1
        );
    }

    #[test]
    fn unread_count_stays_fast_on_populated_mail_table() {
        let store = SqliteStore::open_in_memory().or_panic("store opens");
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
                .or_panic("mail inserts");
        }

        let started = std::time::Instant::now();
        let unread = store
            .count_unread_mail(&recipient_id)
            .or_panic("unread count");

        assert_eq!(unread, 1_000);
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
    }
}
