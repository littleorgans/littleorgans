use std::path::PathBuf;

use chrono::{DateTime, Utc};
use lilo_session_core::{Namespace, Selector};
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
    Core(#[from] lilo_session_core::NamespaceError),
    #[error(transparent)]
    Session(#[from] super::SessionRowError),
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
            sqlx::query("DELETE FROM session_mail WHERE sender_id = ? OR recipient_id = ?")
                .bind(id)
                .bind(id)
                .execute(&mut *transaction)
                .await?;
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

#[cfg(test)]
mod tests {
    use super::super::test_support::running_session;
    use super::*;
    use crate::test_support::OrPanic as _;
    use lilo_session_core::DEFAULT_NAMESPACE;

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
}
