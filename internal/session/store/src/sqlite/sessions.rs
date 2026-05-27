use std::path::PathBuf;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use lilo_session_core::{
    LabelOp, LostEvidence, Namespace, RuntimeKind, Selector, Session, SessionState,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqliteConnection};
use thiserror::Error;
use uuid::Uuid;

use super::SqliteStore;
use super::events::{lost_evidence_from_sql, lost_evidence_to_sql};
use super::time::{parse_optional_timestamp, parse_timestamp};

#[derive(Debug, Error)]
pub enum SessionRowError {
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
    #[error(transparent)]
    Chrono(#[from] chrono::ParseError),
    #[error(transparent)]
    Uuid(#[from] uuid::Error),
    #[error(transparent)]
    Core(#[from] lilo_session_core::SmError),
    #[error(transparent)]
    Namespace(#[from] lilo_session_core::NamespaceError),
    #[error("{field} out of range: {value}")]
    IntegerOutOfRange { field: &'static str, value: i64 },
}

impl SqliteStore {
    pub async fn insert_session(&self, session: &Session) -> Result<(), SessionRowError> {
        insert_session_row(&self.pool, session).await?;
        self.insert_session_labels(&session.id, &session.labels)
            .await?;
        Ok(())
    }

    pub async fn insert_session_in(
        &self,
        conn: &mut SqliteConnection,
        session: &Session,
    ) -> Result<(), SessionRowError> {
        insert_session_row(&mut *conn, session).await?;
        self.insert_session_labels_in(conn, &session.id, &session.labels)
            .await?;
        Ok(())
    }

    pub async fn get_session(&self, id: &Uuid) -> Result<Option<Session>, SessionRowError> {
        let id = id.to_string();
        Ok(self
            .query_sessions("SELECT * FROM session_sessions WHERE id = ?", [id])
            .await?
            .into_iter()
            .next())
    }

    pub async fn list_sessions(&self, id: Option<&str>) -> Result<Vec<Session>, SessionRowError> {
        match id {
            Some(id) => {
                let id = Uuid::parse_str(id)?;
                self.list_sessions_by_selector(&Selector::Id { id }).await
            }
            None => self.list_sessions_by_selector(&Selector::All).await,
        }
    }

    pub async fn list_sessions_by_selector(
        &self,
        selector: &Selector,
    ) -> Result<Vec<Session>, SessionRowError> {
        match selector {
            Selector::All => {
                self.query_sessions(
                    "SELECT * FROM session_sessions ORDER BY created_at",
                    std::iter::empty::<String>(),
                )
                .await
            }
            Selector::Id { id } => {
                self.query_sessions(
                    "SELECT * FROM session_sessions WHERE id = ? ORDER BY created_at",
                    [id.to_string()],
                )
                .await
            }
            Selector::Role { name } => {
                self.query_sessions(
                    "SELECT * FROM session_sessions WHERE role = ? ORDER BY created_at",
                    [name.clone()],
                )
                .await
            }
            Selector::Namespace { namespace } => {
                self.query_sessions(
                    "SELECT * FROM session_sessions WHERE namespace = ? ORDER BY created_at",
                    [namespace.as_str().to_string()],
                )
                .await
            }
            Selector::Dir { path } => {
                self.query_sessions(
                    "SELECT * FROM session_sessions WHERE dir = ? ORDER BY created_at",
                    [path.display().to_string()],
                )
                .await
            }
            Selector::And { selectors } => self.query_and_sessions(selectors).await,
            Selector::Label {
                key,
                op: LabelOp::Eq { value },
            } => {
                self.query_sessions(
                    "SELECT s.*
                 FROM session_sessions s
                 JOIN session_labels l ON l.session_id = s.id
                 WHERE l.key = ? AND l.value = ?
                 ORDER BY s.created_at",
                    [key.clone(), value.clone()],
                )
                .await
            }
            Selector::Label {
                key,
                op: LabelOp::In { values },
            } => self.query_label_in_sessions(key, values).await,
        }
    }

    async fn query_label_in_sessions(
        &self,
        key: &str,
        values: &[String],
    ) -> Result<Vec<Session>, SessionRowError> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = (0..values.len())
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT s.*
             FROM session_sessions s
             JOIN session_labels l ON l.session_id = s.id
             WHERE l.key = ? AND l.value IN ({placeholders})
             ORDER BY s.created_at"
        );
        let params = std::iter::once(key.to_string())
            .chain(values.iter().cloned())
            .collect::<Vec<_>>();
        self.query_sessions(&sql, params).await
    }

    async fn query_and_sessions(
        &self,
        selectors: &[Selector],
    ) -> Result<Vec<Session>, SessionRowError> {
        let mut sessions = self
            .query_sessions(
                "SELECT * FROM session_sessions ORDER BY created_at",
                std::iter::empty::<String>(),
            )
            .await?;
        for selector in selectors {
            sessions.retain(|session| session_matches_selector(session, selector));
        }
        Ok(sessions)
    }

    async fn query_sessions<P>(&self, sql: &str, params: P) -> Result<Vec<Session>, SessionRowError>
    where
        P: IntoIterator,
        P::Item: Into<String>,
    {
        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(param.into());
        }
        let rows = query.fetch_all(&self.pool).await?;
        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(session_from_row(&row)?);
        }
        for session in &mut sessions {
            session.labels = self.labels_for_session(&session.id).await?;
        }
        Ok(sessions)
    }

    pub async fn mark_session_terminated(
        &self,
        id: &Uuid,
        exit_code: Option<i32>,
        terminated_at: DateTime<Utc>,
    ) -> Result<Option<Session>, SessionRowError> {
        sqlx::query(
            "UPDATE session_sessions
             SET state = ?, exit_code = ?, terminated_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(SessionState::Terminated.to_string())
        .bind(exit_code)
        .bind(terminated_at.to_rfc3339())
        .bind(terminated_at.to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        self.get_session(id).await
    }

    pub async fn mark_session_lost(
        &self,
        id: &Uuid,
        evidence: LostEvidence,
        updated_at: DateTime<Utc>,
    ) -> Result<Option<Session>, SessionRowError> {
        sqlx::query(
            "UPDATE session_sessions
             SET state = ?, lost_evidence = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(SessionState::Lost { evidence }.sql_name())
        .bind(lost_evidence_to_sql(evidence))
        .bind(updated_at.to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        self.get_session(id).await
    }

    pub async fn record_transcript_path(
        &self,
        id: &Uuid,
        transcript_path: &std::path::Path,
        updated_at: DateTime<Utc>,
    ) -> Result<Option<Session>, SessionRowError> {
        sqlx::query(
            "UPDATE session_sessions
             SET transcript_path = ?, updated_at = ?
             WHERE id = ?
               AND (transcript_path IS NULL OR transcript_path != ?)",
        )
        .bind(transcript_path.display().to_string())
        .bind(updated_at.to_rfc3339())
        .bind(id.to_string())
        .bind(transcript_path.display().to_string())
        .execute(&self.pool)
        .await?;
        self.get_session(id).await
    }
}

async fn insert_session_row<'e, E>(executor: E, session: &Session) -> Result<(), SessionRowError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO session_sessions
            (id, runtime, role, workspace, namespace, dir, state, lost_evidence, runtime_pid,
             runtime_session, transcript_path, tmux_pane, agent_config, created_at,
             started_at, terminated_at, exit_code, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(session.id.to_string())
    .bind(session.runtime.to_string())
    .bind(&session.role)
    .bind(&session.workspace)
    .bind(session.namespace.as_str())
    .bind(session.dir.display().to_string())
    .bind(session.state.sql_name())
    .bind(session_lost_evidence(session.state))
    .bind(session.runtime_pid)
    .bind(session.runtime_session.as_deref())
    .bind(
        session
            .transcript_path
            .as_ref()
            .map(|path| path.display().to_string()),
    )
    .bind(session.tmux_pane.as_deref())
    .bind(session.agent_config.as_deref())
    .bind(session.created_at.to_rfc3339())
    .bind(session.started_at.to_rfc3339())
    .bind(
        session
            .terminated_at
            .map(|timestamp| timestamp.to_rfc3339()),
    )
    .bind(session.exit_code)
    .bind(session.updated_at.to_rfc3339())
    .execute(executor)
    .await?;
    Ok(())
}

fn session_matches_selector(session: &Session, selector: &Selector) -> bool {
    match selector {
        Selector::All => true,
        Selector::Id { id } => session.id == *id,
        Selector::Role { name } => session.role == *name,
        Selector::Namespace { namespace } => session.namespace == *namespace,
        Selector::Dir { path } => session.dir == *path,
        Selector::And { selectors } => selectors
            .iter()
            .all(|selector| session_matches_selector(session, selector)),
        Selector::Label {
            key,
            op: LabelOp::Eq { value },
        } => session
            .labels
            .iter()
            .any(|label| label.key == *key && label.value == *value),
        Selector::Label {
            key,
            op: LabelOp::In { values },
        } => session
            .labels
            .iter()
            .any(|label| label.key == *key && values.contains(&label.value)),
    }
}

fn session_from_row(row: &SqliteRow) -> Result<Session, SessionRowError> {
    let runtime_pid = row.try_get::<i64, _>("runtime_pid")?;
    let runtime_pid =
        u32::try_from(runtime_pid).map_err(|_| integer_out_of_range("runtime_pid", runtime_pid))?;

    Ok(Session {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)?,
        runtime: RuntimeKind::from_str(&row.try_get::<String, _>("runtime")?)?,
        role: row.try_get("role")?,
        workspace: row.try_get("workspace")?,
        namespace: Namespace::new(row.try_get::<String, _>("namespace")?)?,
        dir: PathBuf::from(row.try_get::<String, _>("dir")?),
        state: session_state_from_row(row)?,
        runtime_pid,
        runtime_session: row.try_get("runtime_session")?,
        transcript_path: row
            .try_get::<Option<String>, _>("transcript_path")?
            .map(Into::into),
        tmux_pane: row.try_get("tmux_pane")?,
        agent_config: row.try_get("agent_config")?,
        created_at: parse_timestamp(&row.try_get::<String, _>("created_at")?)?,
        started_at: parse_timestamp(&row.try_get::<String, _>("started_at")?)?,
        terminated_at: parse_optional_timestamp(
            row.try_get::<Option<String>, _>("terminated_at")?,
        )?,
        exit_code: optional_i32(row, "exit_code")?,
        updated_at: parse_timestamp(&row.try_get::<String, _>("updated_at")?)?,
        labels: Vec::new(),
    })
}

fn session_state_from_row(row: &SqliteRow) -> Result<SessionState, SessionRowError> {
    let lost_evidence = row
        .try_get::<Option<String>, _>("lost_evidence")?
        .as_deref()
        .and_then(lost_evidence_from_sql);
    Ok(SessionState::from_sql(
        &row.try_get::<String, _>("state")?,
        lost_evidence,
    )?)
}

fn session_lost_evidence(state: SessionState) -> Option<&'static str> {
    match state {
        SessionState::Lost { evidence } => Some(lost_evidence_to_sql(evidence)),
        _ => None,
    }
}

fn optional_i32(row: &SqliteRow, column: &'static str) -> Result<Option<i32>, SessionRowError> {
    row.try_get::<Option<i64>, _>(column)?
        .map(|value| i32::try_from(value).map_err(|_| integer_out_of_range(column, value)))
        .transpose()
}

fn integer_out_of_range(field: &'static str, value: i64) -> SessionRowError {
    SessionRowError::IntegerOutOfRange { field, value }
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod sessions_tests;
