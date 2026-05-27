use lilo_session_core::{Label, LabelMutation, Session};
use sqlx::Row;
use uuid::Uuid;

use super::{SessionRowError, SqliteStore};

impl SqliteStore {
    pub async fn apply_label_mutation(
        &self,
        id: &Uuid,
        mutation: &LabelMutation,
    ) -> Result<Option<Session>, SessionRowError> {
        match mutation {
            LabelMutation::Set(label) => self.upsert_label(id, label).await?,
            LabelMutation::Remove { key } => self.remove_label(id, key).await?,
        }
        self.get_session(id).await
    }

    pub(crate) async fn insert_session_labels(
        &self,
        id: &Uuid,
        labels: &[Label],
    ) -> Result<(), SessionRowError> {
        for label in labels {
            self.upsert_label(id, label).await?;
        }
        Ok(())
    }

    pub(crate) async fn labels_for_session(
        &self,
        id: &Uuid,
    ) -> Result<Vec<Label>, SessionRowError> {
        let rows = sqlx::query(
            "SELECT key, value
             FROM session_labels
             WHERE session_id = ?1
             ORDER BY key",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Label {
                    key: row.try_get("key")?,
                    value: row.try_get("value")?,
                })
            })
            .collect()
    }

    async fn upsert_label(&self, id: &Uuid, label: &Label) -> Result<(), SessionRowError> {
        sqlx::query(
            "INSERT INTO session_labels (session_id, key, value)
             VALUES (?, ?, ?)
             ON CONFLICT(session_id, key) DO UPDATE SET value = excluded.value",
        )
        .bind(id.to_string())
        .bind(&label.key)
        .bind(&label.value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn remove_label(&self, id: &Uuid, key: &str) -> Result<(), SessionRowError> {
        sqlx::query("DELETE FROM session_labels WHERE session_id = ? AND key = ?")
            .bind(id.to_string())
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
