use std::collections::BTreeSet;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use lilo_common::id::{MessageId, SessionId};
use lilo_session_core::{Mail, MailIntent, MailStatus, SenderRef, SmError};
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Postgres, QueryBuilder, Row};
use thiserror::Error;

use super::SessionStore;

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

impl SessionStore {
    pub async fn insert_mail(&self, mail: &Mail) -> Result<Mail, MailRowError> {
        let mut inserted = self
            .insert_mail_for_recipients(mail, &[mail.recipient_id])
            .await?;
        Ok(inserted.remove(0))
    }

    pub async fn insert_mail_for_recipients(
        &self,
        mail: &Mail,
        recipient_ids: &[SessionId],
    ) -> Result<Vec<Mail>, MailRowError> {
        Ok(self
            .insert_mail_for_recipients_with_outcome(mail, recipient_ids)
            .await?
            .mail)
    }

    pub async fn insert_mail_for_recipients_with_outcome(
        &self,
        mail: &Mail,
        recipient_ids: &[SessionId],
    ) -> Result<MailWriteOutcome, MailRowError> {
        if recipient_ids.is_empty() {
            return Ok(MailWriteOutcome {
                mail: Vec::new(),
                inserted: false,
            });
        }
        let sender_ref = serde_json::to_string(&mail.sender)?;
        // SELECT-then-INSERT idempotency relies on the partial unique index.
        // Under READ COMMITTED two concurrent senders can both pass the SELECT
        // and race to INSERT; the loser trips the unique constraint. Treat that
        // as the concurrent-duplicate case and retry once, where the SELECT now
        // observes the committed row and collapses to a replay.
        match self
            .try_insert_mail_for_recipients(mail, &sender_ref, recipient_ids)
            .await
        {
            Err(MailRowError::Sqlite(error)) if is_idempotency_conflict(&error) => {
                self.try_insert_mail_for_recipients(mail, &sender_ref, recipient_ids)
                    .await
            }
            other => other,
        }
    }

    async fn try_insert_mail_for_recipients(
        &self,
        mail: &Mail,
        sender_ref: &str,
        recipient_ids: &[SessionId],
    ) -> Result<MailWriteOutcome, MailRowError> {
        let mut transaction = self.pool.begin().await?;
        let result: Result<MailWriteOutcome, MailRowError> = async {
            if let Some(replay) =
                load_idempotent_replay(&mut transaction, sender_ref, mail, recipient_ids).await?
            {
                return Ok(MailWriteOutcome {
                    mail: replay,
                    inserted: false,
                });
            }
            insert_message(&mut transaction, mail, sender_ref).await?;
            insert_deliveries(&mut transaction, mail, recipient_ids).await?;
            Ok(MailWriteOutcome {
                mail: mail_for_recipients(mail, recipient_ids),
                inserted: true,
            })
        }
        .await;
        lilo_db::commit_or_rollback(transaction, result).await
    }

    pub async fn idempotent_mail_for_recipients(
        &self,
        mail: &Mail,
        recipient_ids: &[SessionId],
    ) -> Result<Option<Vec<Mail>>, MailRowError> {
        if recipient_ids.is_empty() || mail.idempotency_key.is_none() {
            return Ok(None);
        }
        let sender_ref = serde_json::to_string(&mail.sender)?;
        let mut conn = self.pool.acquire().await?;
        load_idempotent_replay(&mut conn, &sender_ref, mail, recipient_ids).await
    }

    pub async fn count_unread_mail(&self, recipient_id: &SessionId) -> Result<usize, MailRowError> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM message_deliveries
             WHERE recipient_session_id = $1
               AND status = 'unread'",
        )
        .bind(recipient_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        usize::try_from(count).map_err(|_| integer_out_of_range("unread_count", count))
    }

    pub async fn read_unread_mail(
        &self,
        recipient_id: &SessionId,
        read_at: DateTime<Utc>,
        peek: bool,
    ) -> Result<Vec<Mail>, MailRowError> {
        if peek {
            return fetch_unread(&self.pool, recipient_id).await;
        }

        let mut transaction = self.pool.begin().await?;
        let result: Result<Vec<Mail>, MailRowError> = async {
            let mail = fetch_unread(&mut *transaction, recipient_id).await?;
            for item in &mail {
                sqlx::query(
                    "UPDATE message_deliveries
                     SET status = 'read', read_at = $1
                     WHERE message_id = $2
                       AND recipient_session_id = $3
                       AND status = 'unread'",
                )
                .bind(read_at)
                .bind(item.id.to_string())
                .bind(recipient_id.to_string())
                .execute(&mut *transaction)
                .await?;
            }
            Ok(mail)
        }
        .await;
        let mut mail = lilo_db::commit_or_rollback(transaction, result).await?;
        for item in &mut mail {
            item.read_at = Some(read_at);
        }
        Ok(mail)
    }

    pub async fn count_conversation_depth(&self, context_id: &str) -> Result<usize, MailRowError> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM messages
             WHERE context_id = $1
               AND intent != 'receipt'",
        )
        .bind(context_id)
        .fetch_one(&self.pool)
        .await?;
        usize::try_from(count).map_err(|_| integer_out_of_range("conversation_depth", count))
    }

    pub async fn count_sender_rate_since(
        &self,
        sender: &SenderRef,
        since: DateTime<Utc>,
    ) -> Result<usize, MailRowError> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM messages
             WHERE sender_ref = $1
               AND intent != 'receipt'
               AND sent_at >= $2",
        )
        .bind(serde_json::to_string(sender)?)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        usize::try_from(count).map_err(|_| integer_out_of_range("sender_rate", count))
    }

    pub async fn list_message_log(
        &self,
        context_id: Option<&str>,
        participant_ids: Option<&[SessionId]>,
        recipient_ids: Option<&[SessionId]>,
        include_system: bool,
        after: Option<(&DateTime<Utc>, &MessageId)>,
    ) -> Result<Vec<Mail>, MailRowError> {
        if participant_ids.is_some_and(<[SessionId]>::is_empty)
            || recipient_ids.is_some_and(<[SessionId]>::is_empty)
        {
            return Ok(Vec::new());
        }

        let sender_refs = participant_sender_refs(participant_ids)?;
        let mut query = QueryBuilder::<Postgres>::new(MESSAGE_LOG_SELECT_SQL);
        if let Some(context_id) = context_id {
            query.push(" AND m.context_id = ");
            query.push_bind(context_id);
        }
        if !include_system {
            query.push(" AND m.intent != 'receipt'");
        }
        if let Some(participant_ids) = participant_ids {
            query.push(" AND (d.recipient_session_id IN (");
            push_session_id_binds(&mut query, participant_ids);
            query.push(") OR m.sender_ref IN (");
            push_string_binds(&mut query, &sender_refs);
            query.push("))");
        }
        if let Some(recipient_ids) = recipient_ids {
            query.push(" AND d.recipient_session_id IN (");
            push_session_id_binds(&mut query, recipient_ids);
            query.push(")");
        }
        if let Some((sent_at, message_id)) = after {
            let sent_at = *sent_at;
            query.push(" AND (m.sent_at > ");
            query.push_bind(sent_at);
            query.push(" OR (m.sent_at = ");
            query.push_bind(sent_at);
            query.push(" AND m.message_id > ");
            query.push_bind(message_id.to_string());
            query.push("))");
        }
        query.push(" ORDER BY m.sent_at, m.message_id, d.recipient_session_id");

        let rows = query.build().fetch_all(&self.pool).await?;
        rows.iter().map(mail_from_row).collect()
    }
}

pub struct MailWriteOutcome {
    pub mail: Vec<Mail>,
    pub inserted: bool,
}

async fn insert_message(
    transaction: &mut PgConnection,
    mail: &Mail,
    sender_ref: &str,
) -> Result<(), MailRowError> {
    sqlx::query(
        "INSERT INTO messages
         (message_id, sender_ref, context_id, intent, idempotency_key, content, sent_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(mail.id.to_string())
    .bind(sender_ref)
    .bind(&mail.context_id)
    .bind(mail.intent.to_string())
    .bind(&mail.idempotency_key)
    .bind(&mail.content)
    .bind(mail.sent_at)
    .execute(&mut *transaction)
    .await?;
    Ok(())
}

async fn insert_deliveries(
    transaction: &mut PgConnection,
    mail: &Mail,
    recipient_ids: &[SessionId],
) -> Result<(), MailRowError> {
    for recipient_id in recipient_ids {
        sqlx::query(
            "INSERT INTO message_deliveries
             (message_id, recipient_session_id, status, read_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(mail.id.to_string())
        .bind(recipient_id.to_string())
        .bind(mail.status.to_string())
        .bind(mail.read_at)
        .execute(&mut *transaction)
        .await?;
    }
    Ok(())
}

fn mail_from_row(row: &PgRow) -> Result<Mail, MailRowError> {
    Ok(Mail {
        id: row.try_get::<String, _>("id")?.parse()?,
        sender: serde_json::from_str::<SenderRef>(&row.try_get::<String, _>("sender_ref")?)?,
        recipient_id: row.try_get::<String, _>("recipient_id")?.parse()?,
        content: row.try_get("content")?,
        sent_at: row.try_get::<DateTime<Utc>, _>("sent_at")?,
        read_at: row.try_get::<Option<DateTime<Utc>>, _>("read_at")?,
        status: MailStatus::from_str(&row.try_get::<String, _>("status")?)?,
        context_id: row.try_get("context_id")?,
        intent: MailIntent::from_str(&row.try_get::<String, _>("intent")?)?,
        idempotency_key: row.try_get("idempotency_key")?,
    })
}

#[derive(Clone)]
struct StoredMessage {
    id: MessageId,
    sender: SenderRef,
    content: String,
    sent_at: DateTime<Utc>,
    context_id: String,
    intent: MailIntent,
    idempotency_key: Option<String>,
}

impl StoredMessage {
    fn to_mail(
        &self,
        recipient_id: SessionId,
        read_at: Option<DateTime<Utc>>,
        status: MailStatus,
    ) -> Mail {
        Mail {
            id: self.id,
            sender: self.sender.clone(),
            recipient_id,
            content: self.content.clone(),
            sent_at: self.sent_at,
            read_at,
            status,
            context_id: self.context_id.clone(),
            intent: self.intent,
            idempotency_key: self.idempotency_key.clone(),
        }
    }
}

async fn load_idempotent_replay(
    transaction: &mut PgConnection,
    sender_ref: &str,
    mail: &Mail,
    recipient_ids: &[SessionId],
) -> Result<Option<Vec<Mail>>, MailRowError> {
    let Some(key) = &mail.idempotency_key else {
        return Ok(None);
    };
    let Some(existing) = message_by_idempotency(transaction, sender_ref, key).await? else {
        return Ok(None);
    };
    validate_idempotent_replay(transaction, &existing, mail, recipient_ids).await?;
    Ok(Some(
        load_message_deliveries(transaction, &existing, recipient_ids).await?,
    ))
}

async fn message_by_idempotency(
    transaction: &mut PgConnection,
    sender_ref: &str,
    key: &str,
) -> Result<Option<StoredMessage>, MailRowError> {
    sqlx::query(
        "SELECT message_id AS id, sender_ref, content, sent_at, context_id, intent, idempotency_key
         FROM messages
         WHERE sender_ref = $1
           AND idempotency_key = $2",
    )
    .bind(sender_ref)
    .bind(key)
    .fetch_optional(&mut *transaction)
    .await?
    .map(|row| stored_message_from_row(&row))
    .transpose()
}

async fn validate_idempotent_replay(
    transaction: &mut PgConnection,
    existing: &StoredMessage,
    mail: &Mail,
    recipient_ids: &[SessionId],
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
    transaction: &mut PgConnection,
    message_id: &MessageId,
) -> Result<BTreeSet<String>, MailRowError> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT recipient_session_id
         FROM message_deliveries
         WHERE message_id = $1",
    )
    .bind(message_id.to_string())
    .fetch_all(&mut *transaction)
    .await?;
    Ok(rows.into_iter().collect())
}

fn recipient_set(recipient_ids: &[SessionId]) -> BTreeSet<String> {
    recipient_ids.iter().map(ToString::to_string).collect()
}

async fn load_message_deliveries(
    transaction: &mut PgConnection,
    message: &StoredMessage,
    recipient_ids: &[SessionId],
) -> Result<Vec<Mail>, MailRowError> {
    let mut mail = Vec::new();
    for recipient_id in recipient_ids {
        let row = sqlx::query(
            "SELECT read_at, status
             FROM message_deliveries
             WHERE message_id = $1
               AND recipient_session_id = $2",
        )
        .bind(message.id.to_string())
        .bind(recipient_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        let read_at = row.try_get::<Option<DateTime<Utc>>, _>("read_at")?;
        let status = MailStatus::from_str(&row.try_get::<String, _>("status")?)?;
        mail.push(message.to_mail(*recipient_id, read_at, status));
    }
    Ok(mail)
}

fn mail_for_recipients(mail: &Mail, recipient_ids: &[SessionId]) -> Vec<Mail> {
    recipient_ids
        .iter()
        .map(|recipient_id| Mail {
            recipient_id: *recipient_id,
            ..mail.clone()
        })
        .collect()
}

/// Mark every unread delivery addressed to `recipient_id` as undeliverable.
/// Called when a recipient session reaches a terminal state (terminated or
/// lost) and can no longer read its mail, so the unread count stays honest
/// while the transcript still shows the dropped delivery. Read deliveries are
/// left untouched.
pub(super) async fn mark_unread_undeliverable<'e, E>(
    executor: E,
    recipient_id: &SessionId,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "UPDATE message_deliveries
         SET status = 'undeliverable'
         WHERE recipient_session_id = $1
           AND status = 'unread'",
    )
    .bind(recipient_id.to_string())
    .execute(executor)
    .await?;
    Ok(())
}

async fn fetch_unread<'e, E>(
    executor: E,
    recipient_id: &SessionId,
) -> Result<Vec<Mail>, MailRowError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let rows = sqlx::query(UNREAD_MAIL_SQL)
        .bind(recipient_id.to_string())
        .fetch_all(executor)
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
           d.status,
           m.context_id,
           m.intent,
           m.idempotency_key
    FROM message_deliveries d
    JOIN messages m ON m.message_id = d.message_id
    WHERE d.recipient_session_id = $1
      AND d.status = 'unread'
    ORDER BY m.sent_at, m.message_id
";

const MESSAGE_LOG_SELECT_SQL: &str = "
    SELECT m.message_id AS id,
           m.sender_ref,
           d.recipient_session_id AS recipient_id,
           m.content,
           m.sent_at,
           d.read_at,
           d.status,
           m.context_id,
           m.intent,
           m.idempotency_key
    FROM messages m
    JOIN message_deliveries d ON d.message_id = m.message_id
    WHERE 1 = 1
";

fn stored_message_from_row(row: &PgRow) -> Result<StoredMessage, MailRowError> {
    Ok(StoredMessage {
        id: row.try_get::<String, _>("id")?.parse()?,
        sender: serde_json::from_str::<SenderRef>(&row.try_get::<String, _>("sender_ref")?)?,
        content: row.try_get("content")?,
        sent_at: row.try_get::<DateTime<Utc>, _>("sent_at")?,
        context_id: row.try_get("context_id")?,
        intent: MailIntent::from_str(&row.try_get::<String, _>("intent")?)?,
        idempotency_key: row.try_get("idempotency_key")?,
    })
}

/// True when `error` is a unique-constraint violation on the message
/// idempotency index, i.e. a concurrent sender won the INSERT race. Scoped to
/// that constraint so other unique violations still surface as hard errors.
fn is_idempotency_conflict(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|db| db.is_unique_violation() && db.constraint() == Some(IDEMPOTENCY_INDEX))
}

const IDEMPOTENCY_INDEX: &str = "idx_messages_sender_idempotency";

fn integer_out_of_range(field: &'static str, value: i64) -> MailRowError {
    MailRowError::IntegerOutOfRange { field, value }
}

fn participant_sender_refs(
    participant_ids: Option<&[SessionId]>,
) -> Result<Vec<String>, MailRowError> {
    participant_ids
        .unwrap_or_default()
        .iter()
        .map(|id| serde_json::to_string(&SenderRef::session(*id)).map_err(Into::into))
        .collect()
}

fn push_session_id_binds(query: &mut QueryBuilder<'_, Postgres>, ids: &[SessionId]) {
    let mut separated = query.separated(", ");
    for id in ids {
        separated.push_bind(id.to_string());
    }
}

fn push_string_binds<'q>(query: &mut QueryBuilder<'q, Postgres>, values: &'q [String]) {
    let mut separated = query.separated(", ");
    for value in values {
        separated.push_bind(value);
    }
}
