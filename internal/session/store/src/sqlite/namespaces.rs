use std::path::PathBuf;

use chrono::{DateTime, Utc};
use lilo_session_core::{Namespace, Selector};
use rusqlite::{OptionalExtension, params};
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
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Chrono(#[from] chrono::ParseError),
    #[error(transparent)]
    Core(#[from] lilo_session_core::NamespaceError),
    #[error(transparent)]
    Session(#[from] super::SessionRowError),
}

impl SqliteStore {
    pub fn namespace_exists(&self, namespace: &Namespace) -> Result<bool, NamespaceRowError> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM namespaces WHERE slug = ?1)",
            [namespace.as_str()],
            |row| row.get(0),
        )?)
    }

    pub fn create_namespace(
        &self,
        namespace: &Namespace,
        created_at: DateTime<Utc>,
    ) -> Result<(), NamespaceRowError> {
        self.connection.execute(
            "INSERT INTO namespaces (slug, created_at)
             VALUES (?1, ?2)",
            params![namespace.as_str(), created_at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn delete_namespace(&self, namespace: &Namespace) -> Result<bool, NamespaceRowError> {
        let changed = self.connection.execute(
            "DELETE FROM namespaces WHERE slug = ?1",
            params![namespace.as_str()],
        )?;
        Ok(changed > 0)
    }

    pub fn delete_sessions_by_namespace(
        &self,
        namespace: &Namespace,
    ) -> Result<usize, NamespaceRowError> {
        let session_ids = self
            .list_sessions_by_selector(&Selector::Namespace {
                namespace: namespace.clone(),
            })?
            .into_iter()
            .map(|session| session.id.to_string())
            .collect::<Vec<_>>();
        for id in &session_ids {
            self.connection
                .execute("DELETE FROM labels WHERE session_id = ?1", params![id])?;
            self.connection.execute(
                "DELETE FROM mail WHERE sender_id = ?1 OR recipient_id = ?1",
                params![id],
            )?;
        }
        self.connection.execute(
            "DELETE FROM sessions WHERE namespace = ?1",
            params![namespace.as_str()],
        )?;
        Ok(session_ids.len())
    }

    pub fn active_session_count_in_namespace(
        &self,
        namespace: &Namespace,
    ) -> Result<usize, NamespaceRowError> {
        Ok(self
            .list_sessions_by_selector(&Selector::Namespace {
                namespace: namespace.clone(),
            })?
            .into_iter()
            .filter(|session| session.state.is_active())
            .count())
    }

    pub fn list_namespaces(&self) -> Result<Vec<NamespaceRecord>, NamespaceRowError> {
        let mut statement = self
            .connection
            .prepare("SELECT slug, created_at FROM namespaces ORDER BY slug")?;
        let mut rows = statement.query([])?;
        let mut namespaces = Vec::new();
        while let Some(row) = rows.next()? {
            namespaces.push(NamespaceRecord {
                namespace: Namespace::new(row.get::<_, String>("slug")?)?,
                created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
            });
        }
        Ok(namespaces)
    }

    pub fn get_session_namespace(
        &self,
        id: &Uuid,
    ) -> Result<Option<SessionNamespace>, NamespaceRowError> {
        let raw = self
            .connection
            .query_row(
                "SELECT namespace, dir FROM sessions WHERE id = ?1",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>("namespace")?,
                        row.get::<_, String>("dir")?,
                    ))
                },
            )
            .optional()?;
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
    use super::*;
    use crate::test_support::OrPanic as _;
    use lilo_session_core::{DEFAULT_NAMESPACE, RuntimeKind, Session, SessionState};

    #[test]
    fn seeds_default_namespace_and_session_location() {
        let store = SqliteStore::open_in_memory().or_panic("store opens");
        let default_namespace = Namespace::default();
        let session = test_session("/tmp/project");

        assert!(
            store
                .namespace_exists(&default_namespace)
                .or_panic("namespace exists")
        );
        assert_eq!(
            store
                .list_namespaces()
                .or_panic("namespaces list")
                .into_iter()
                .map(|record| record.namespace)
                .collect::<Vec<_>>(),
            vec![default_namespace.clone()]
        );

        store.insert_session(&session).or_panic("session inserts");
        assert_eq!(
            store
                .get_session_namespace(&session.id)
                .or_panic("session namespace loads"),
            Some(SessionNamespace {
                namespace: default_namespace,
                dir: PathBuf::from("/tmp/project"),
            })
        );
    }

    #[test]
    fn creates_and_lists_namespaces() {
        let store = SqliteStore::open_in_memory().or_panic("store opens");
        let namespace = Namespace::for_create("alpha").or_panic("namespace validates");
        let created_at = Utc::now();

        assert!(
            !store
                .namespace_exists(&namespace)
                .or_panic("namespace checks")
        );
        store
            .create_namespace(&namespace, created_at)
            .or_panic("namespace creates");
        assert!(
            store
                .namespace_exists(&namespace)
                .or_panic("namespace checks")
        );

        let records = store.list_namespaces().or_panic("namespaces list");
        assert_eq!(
            records
                .iter()
                .map(|record| record.namespace.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", DEFAULT_NAMESPACE]
        );
    }

    fn test_session(workspace: &str) -> Session {
        let now = Utc::now();
        Session {
            id: Uuid::now_v7(),
            runtime: RuntimeKind::Claude,
            role: "engineer".to_string(),
            workspace: workspace.to_string(),
            namespace: Namespace::default(),
            dir: PathBuf::from(workspace),
            state: SessionState::Running,
            runtime_pid: 42,
            runtime_session: None,
            transcript_path: None,
            tmux_pane: None,
            agent_config: None,
            created_at: now,
            started_at: now,
            terminated_at: None,
            exit_code: None,
            updated_at: now,
            labels: Vec::new(),
        }
    }
}
