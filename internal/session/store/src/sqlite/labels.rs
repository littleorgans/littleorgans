use lilo_session_core::{Label, LabelMutation, Session};
use rusqlite::params;
use uuid::Uuid;

use super::{SessionRowError, SqliteStore};

impl SqliteStore {
    pub fn apply_label_mutation(
        &self,
        id: &Uuid,
        mutation: &LabelMutation,
    ) -> Result<Option<Session>, SessionRowError> {
        match mutation {
            LabelMutation::Set(label) => self.upsert_label(id, label)?,
            LabelMutation::Remove { key } => self.remove_label(id, key)?,
        }
        self.get_session(id)
    }

    pub(crate) fn insert_session_labels(
        &self,
        id: &Uuid,
        labels: &[Label],
    ) -> Result<(), SessionRowError> {
        for label in labels {
            self.upsert_label(id, label)?;
        }
        Ok(())
    }

    pub(crate) fn labels_for_session(&self, id: &Uuid) -> Result<Vec<Label>, SessionRowError> {
        let mut statement = self.connection.prepare(
            "SELECT key, value
             FROM labels
             WHERE session_id = ?1
             ORDER BY key",
        )?;
        let rows = statement.query_map([id.to_string()], |row| {
            Ok(Label {
                key: row.get("key")?,
                value: row.get("value")?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn upsert_label(&self, id: &Uuid, label: &Label) -> Result<(), SessionRowError> {
        self.connection.execute(
            "INSERT INTO labels (session_id, key, value)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id, key) DO UPDATE SET value = excluded.value",
            params![id.to_string(), &label.key, &label.value],
        )?;
        Ok(())
    }

    fn remove_label(&self, id: &Uuid, key: &str) -> Result<(), SessionRowError> {
        self.connection.execute(
            "DELETE FROM labels WHERE session_id = ?1 AND key = ?2",
            params![id.to_string(), key],
        )?;
        Ok(())
    }
}
