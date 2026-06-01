use std::path::PathBuf;

use chrono::{DateTime, Utc};
use lilo_session_core::{Namespace, Selector, SenderRef};
use sqlx::Row;
use thiserror::Error;
use uuid::Uuid;

use super::SqliteStore;
use super::time::parse_timestamp;

pub use lilo_session_core::NamespaceRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionNamespace {
    pub namespace: Namespace,
    pub dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum NamespaceRowError {
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
    #[error(transparent)]
    Chrono(#[from] chrono::ParseError),
    #[error(transparent)]
    Uuid(#[from] uuid::Error),
    #[error(transparent)]
    Core(#[from] lilo_session_core::NamespaceError),
    #[error(transparent)]
    Session(#[from] super::SessionRowError),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
}

impl SqliteStore {
    pub async fn namespace_exists(&self, namespace: &Namespace) -> Result<bool, NamespaceRowError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM session_namespaces WHERE slug = ?)",
        )
        .bind(namespace.as_str())
        .fetch_one(&self.pool)
        .await?;
        Ok(exists != 0)
    }

    pub async fn create_namespace(
        &self,
        namespace: &Namespace,
        created_at: DateTime<Utc>,
    ) -> Result<(), NamespaceRowError> {
        sqlx::query(
            "INSERT INTO session_namespaces (slug, created_at)
             VALUES (?, ?)",
        )
        .bind(namespace.as_str())
        .bind(created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_namespace(&self, namespace: &Namespace) -> Result<bool, NamespaceRowError> {
        let changed = sqlx::query("DELETE FROM session_namespaces WHERE slug = ?")
            .bind(namespace.as_str())
            .execute(&self.pool)
            .await?;
        Ok(changed.rows_affected() > 0)
    }

    pub async fn delete_sessions_by_namespace(
        &self,
        namespace: &Namespace,
    ) -> Result<usize, NamespaceRowError> {
        let session_ids = self
            .list_sessions_by_selector(&Selector::Namespace {
                namespace: namespace.clone(),
            })
            .await?
            .into_iter()
            .map(|session| session.id.to_string())
            .collect::<Vec<_>>();
        let mut transaction = self.pool.begin().await?;
        for id in &session_ids {
            sqlx::query("DELETE FROM session_labels WHERE session_id = ?")
                .bind(id)
                .execute(&mut *transaction)
                .await?;
            delete_recipient_deliveries(&mut transaction, id).await?;
        }
        for id in &session_ids {
            gc_session_sender_messages(&mut transaction, id).await?;
        }
        sqlx::query("DELETE FROM session_sessions WHERE namespace = ?")
            .bind(namespace.as_str())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(session_ids.len())
    }

    pub async fn active_session_count_in_namespace(
        &self,
        namespace: &Namespace,
    ) -> Result<usize, NamespaceRowError> {
        Ok(self
            .list_sessions_by_selector(&Selector::Namespace {
                namespace: namespace.clone(),
            })
            .await?
            .into_iter()
            .filter(|session| session.state.is_active())
            .count())
    }

    pub async fn list_namespaces(&self) -> Result<Vec<NamespaceRecord>, NamespaceRowError> {
        let rows = sqlx::query("SELECT slug, created_at FROM session_namespaces ORDER BY slug")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(NamespaceRecord {
                    namespace: Namespace::new(row.try_get::<String, _>("slug")?)?,
                    created_at: parse_timestamp(&row.try_get::<String, _>("created_at")?)?,
                })
            })
            .collect()
    }

    pub async fn get_session_namespace(
        &self,
        id: &Uuid,
    ) -> Result<Option<SessionNamespace>, NamespaceRowError> {
        let raw = sqlx::query("SELECT namespace, dir FROM session_sessions WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .map(|row| {
                Ok::<_, sqlx::Error>((
                    row.try_get::<String, _>("namespace")?,
                    row.try_get::<String, _>("dir")?,
                ))
            })
            .transpose()?;
        raw.map(|(namespace, dir)| {
            Ok(SessionNamespace {
                namespace: Namespace::new(namespace)?,
                dir: PathBuf::from(dir),
            })
        })
        .transpose()
    }
}

async fn delete_recipient_deliveries(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session_id: &str,
) -> Result<(), NamespaceRowError> {
    sqlx::query("DELETE FROM message_deliveries WHERE recipient_session_id = ?")
        .bind(session_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn gc_session_sender_messages(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session_id: &str,
) -> Result<(), NamespaceRowError> {
    let sender_ref = serde_json::to_string(&SenderRef::session(Uuid::parse_str(session_id)?))?;
    sqlx::query(
        "DELETE FROM messages
         WHERE sender_ref = ?
           AND NOT EXISTS (
               SELECT 1
               FROM message_deliveries d
               WHERE d.message_id = messages.message_id
           )",
    )
    .bind(sender_ref)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::running_session;
    use super::*;
    use crate::test_support::OrPanic as _;
    use lilo_session_core::{DEFAULT_NAMESPACE, Mail, MailIntent};
    use serde_json::json;

    #[tokio::test]
    async fn seeds_default_namespace_and_session_location() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let default_namespace = Namespace::default();
        let session = running_session("engineer", "/tmp/project");

        assert!(
            store
                .namespace_exists(&default_namespace)
                .await
                .or_panic("namespace exists")
        );
        assert_eq!(
            store
                .list_namespaces()
                .await
                .or_panic("namespaces list")
                .into_iter()
                .map(|record| record.namespace)
                .collect::<Vec<_>>(),
            vec![default_namespace.clone()]
        );

        store
            .insert_session(&session)
            .await
            .or_panic("session inserts");
        assert_eq!(
            store
                .get_session_namespace(&session.id)
                .await
                .or_panic("session namespace loads"),
            Some(SessionNamespace {
                namespace: default_namespace,
                dir: PathBuf::from("/tmp/project"),
            })
        );
    }

    #[tokio::test]
    async fn creates_and_lists_namespaces() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let namespace = Namespace::for_create("alpha").or_panic("namespace validates");
        let created_at = Utc::now();

        assert!(
            !store
                .namespace_exists(&namespace)
                .await
                .or_panic("namespace checks")
        );
        store
            .create_namespace(&namespace, created_at)
            .await
            .or_panic("namespace creates");
        assert!(
            store
                .namespace_exists(&namespace)
                .await
                .or_panic("namespace checks")
        );

        let records = store.list_namespaces().await.or_panic("namespaces list");
        assert_eq!(
            records
                .iter()
                .map(|record| record.namespace.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", DEFAULT_NAMESPACE]
        );
    }

    #[tokio::test]
    async fn namespace_delete_splits_delivery_cleanup_from_message_log_gc() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let namespace = Namespace::for_create("alpha").or_panic("namespace validates");
        store
            .create_namespace(&namespace, Utc::now())
            .await
            .or_panic("namespace creates");
        let mut sender = running_session("pm", "/tmp/alpha-sender");
        sender.namespace = namespace.clone();
        let mut recipient = running_session("engineer", "/tmp/alpha-recipient");
        recipient.namespace = namespace.clone();
        let outside = running_session("reviewer", "/tmp/default-recipient");
        for session in [&sender, &recipient, &outside] {
            store
                .insert_session(session)
                .await
                .or_panic("session inserts");
        }
        let surviving = mail_from(
            SenderRef::session(sender.id),
            outside.id,
            "survives through outside delivery",
        );
        let gc_candidate = mail_from(
            SenderRef::session(sender.id),
            recipient.id,
            "removed after sender deletion and no deliveries survive",
        );
        let operator = mail_from(
            SenderRef::operator(json!({"kind": "local", "uid": 42})),
            recipient.id,
            "operator transcript remains host anchored",
        );
        store
            .insert_mail_for_recipients(&surviving, &[recipient.id, outside.id])
            .await
            .or_panic("surviving mail inserts");
        store
            .insert_mail(&gc_candidate)
            .await
            .or_panic("gc mail inserts");
        store
            .insert_mail(&operator)
            .await
            .or_panic("operator mail inserts");

        assert_eq!(
            store
                .delete_sessions_by_namespace(&namespace)
                .await
                .or_panic("namespace sessions delete"),
            2
        );

        assert_eq!(delivery_count_for(&store, recipient.id).await, 0);
        assert!(message_exists(&store, surviving.id).await);
        assert!(!message_exists(&store, gc_candidate.id).await);
        assert!(message_exists(&store, operator.id).await);
    }

    fn mail_from(sender: SenderRef, recipient_id: Uuid, content: &str) -> Mail {
        Mail {
            id: Uuid::now_v7(),
            sender,
            recipient_id,
            content: content.to_string(),
            sent_at: Utc::now(),
            read_at: None,
            context_id: "namespace-thread".to_string(),
            intent: MailIntent::Inform,
            idempotency_key: None,
        }
    }

    async fn delivery_count_for(store: &SqliteStore, recipient_id: Uuid) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM message_deliveries
             WHERE recipient_session_id = ?",
        )
        .bind(recipient_id.to_string())
        .fetch_one(store.pool())
        .await
        .or_panic("delivery count")
    }

    async fn message_exists(store: &SqliteStore, message_id: Uuid) -> bool {
        sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(
                SELECT 1
                FROM messages
                WHERE message_id = ?
             )",
        )
        .bind(message_id.to_string())
        .fetch_one(store.pool())
        .await
        .or_panic("message exists")
            != 0
    }
}
