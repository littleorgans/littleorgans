use std::path::PathBuf;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use lilo_session_core::{
    LabelOp, LostEvidence, Namespace, RuntimeKind, Selector, Session, SessionState,
};
use rusqlite::{Row, params, params_from_iter};
use thiserror::Error;
use uuid::Uuid;

use super::SqliteStore;
use super::events::{lost_evidence_from_sql, lost_evidence_to_sql};
use super::time::{parse_optional_timestamp, parse_timestamp};

#[derive(Debug, Error)]
pub enum SessionRowError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
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
    pub fn insert_session(&self, session: &Session) -> Result<(), SessionRowError> {
        self.connection.execute(
            "INSERT INTO sessions
                (id, runtime, role, workspace, namespace, dir, state, lost_evidence, runtime_pid,
                 runtime_session, transcript_path, tmux_pane, agent_config, created_at,
                 started_at, terminated_at, exit_code, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                session.id.to_string(),
                session.runtime.to_string(),
                &session.role,
                &session.workspace,
                session.namespace.as_str(),
                session.dir.display().to_string(),
                session.state.sql_name(),
                session_lost_evidence(session.state),
                session.runtime_pid,
                session.runtime_session.as_deref(),
                session
                    .transcript_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                session.tmux_pane.as_deref(),
                session.agent_config.as_deref(),
                session.created_at.to_rfc3339(),
                session.started_at.to_rfc3339(),
                session
                    .terminated_at
                    .map(|timestamp| timestamp.to_rfc3339()),
                session.exit_code,
                session.updated_at.to_rfc3339(),
            ],
        )?;
        self.insert_session_labels(&session.id, &session.labels)?;
        Ok(())
    }

    pub fn get_session(&self, id: &Uuid) -> Result<Option<Session>, SessionRowError> {
        let id = id.to_string();
        Ok(self
            .query_sessions("SELECT * FROM sessions WHERE id = ?1", [&id])?
            .into_iter()
            .next())
    }

    pub fn list_sessions(&self, id: Option<&str>) -> Result<Vec<Session>, SessionRowError> {
        match id {
            Some(id) => {
                let id = Uuid::parse_str(id)?;
                self.list_sessions_by_selector(&Selector::Id { id })
            }
            None => self.list_sessions_by_selector(&Selector::All),
        }
    }

    pub fn list_sessions_by_selector(
        &self,
        selector: &Selector,
    ) -> Result<Vec<Session>, SessionRowError> {
        match selector {
            Selector::All => self.query_sessions(
                "SELECT * FROM sessions ORDER BY created_at",
                std::iter::empty::<String>(),
            ),
            Selector::Id { id } => self.query_sessions(
                "SELECT * FROM sessions WHERE id = ?1 ORDER BY created_at",
                [id.to_string()],
            ),
            Selector::Role { name } => self.query_sessions(
                "SELECT * FROM sessions WHERE role = ?1 ORDER BY created_at",
                [name.clone()],
            ),
            Selector::Namespace { namespace } => self.query_sessions(
                "SELECT * FROM sessions WHERE namespace = ?1 ORDER BY created_at",
                [namespace.as_str().to_string()],
            ),
            Selector::Dir { path } => self.query_sessions(
                "SELECT * FROM sessions WHERE dir = ?1 ORDER BY created_at",
                [path.display().to_string()],
            ),
            Selector::And { selectors } => self.query_and_sessions(selectors),
            Selector::Label {
                key,
                op: LabelOp::Eq { value },
            } => self.query_sessions(
                "SELECT s.*
                 FROM sessions s
                 JOIN labels l ON l.session_id = s.id
                 WHERE l.key = ?1 AND l.value = ?2
                 ORDER BY s.created_at",
                [key.clone(), value.clone()],
            ),
            Selector::Label {
                key,
                op: LabelOp::In { values },
            } => self.query_label_in_sessions(key, values),
        }
    }

    fn query_label_in_sessions(
        &self,
        key: &str,
        values: &[String],
    ) -> Result<Vec<Session>, SessionRowError> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = (2..values.len() + 2)
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT s.*
             FROM sessions s
             JOIN labels l ON l.session_id = s.id
             WHERE l.key = ?1 AND l.value IN ({placeholders})
             ORDER BY s.created_at"
        );
        let params = std::iter::once(key.to_string())
            .chain(values.iter().cloned())
            .collect::<Vec<_>>();
        self.query_sessions(&sql, params)
    }

    fn query_and_sessions(&self, selectors: &[Selector]) -> Result<Vec<Session>, SessionRowError> {
        let mut sessions = self.list_sessions_by_selector(&Selector::All)?;
        for selector in selectors {
            sessions.retain(|session| session_matches_selector(session, selector));
        }
        Ok(sessions)
    }

    fn query_sessions<P>(&self, sql: &str, params: P) -> Result<Vec<Session>, SessionRowError>
    where
        P: IntoIterator,
        P::Item: rusqlite::ToSql,
    {
        let mut statement = self.connection.prepare(sql)?;
        let mut rows = statement.query(params_from_iter(params))?;
        let mut sessions = Vec::new();
        while let Some(row) = rows.next()? {
            sessions.push(session_from_row(row)?);
        }
        drop(rows);
        drop(statement);
        for session in &mut sessions {
            session.labels = self.labels_for_session(&session.id)?;
        }
        Ok(sessions)
    }

    pub fn mark_session_terminated(
        &self,
        id: &Uuid,
        exit_code: Option<i32>,
        terminated_at: DateTime<Utc>,
    ) -> Result<Option<Session>, SessionRowError> {
        self.connection.execute(
            "UPDATE sessions
             SET state = ?1, exit_code = ?2, terminated_at = ?3, updated_at = ?4
             WHERE id = ?5",
            params![
                SessionState::Terminated.to_string(),
                exit_code,
                terminated_at.to_rfc3339(),
                terminated_at.to_rfc3339(),
                id.to_string(),
            ],
        )?;
        self.get_session(id)
    }

    pub fn mark_session_lost(
        &self,
        id: &Uuid,
        evidence: LostEvidence,
        updated_at: DateTime<Utc>,
    ) -> Result<Option<Session>, SessionRowError> {
        self.connection.execute(
            "UPDATE sessions
             SET state = ?1, lost_evidence = ?2, updated_at = ?3
             WHERE id = ?4",
            params![
                SessionState::Lost { evidence }.sql_name(),
                lost_evidence_to_sql(evidence),
                updated_at.to_rfc3339(),
                id.to_string(),
            ],
        )?;
        self.get_session(id)
    }

    pub fn record_transcript_path(
        &self,
        id: &Uuid,
        transcript_path: &std::path::Path,
        updated_at: DateTime<Utc>,
    ) -> Result<Option<Session>, SessionRowError> {
        self.connection.execute(
            "UPDATE sessions
             SET transcript_path = ?1, updated_at = ?2
             WHERE id = ?3
               AND (transcript_path IS NULL OR transcript_path != ?1)",
            params![
                transcript_path.display().to_string(),
                updated_at.to_rfc3339(),
                id.to_string(),
            ],
        )?;
        self.get_session(id)
    }
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

fn session_from_row(row: &Row<'_>) -> Result<Session, SessionRowError> {
    let runtime_pid = row.get::<_, i64>("runtime_pid")?;
    let runtime_pid =
        u32::try_from(runtime_pid).map_err(|_| integer_out_of_range("runtime_pid", runtime_pid))?;

    Ok(Session {
        id: Uuid::parse_str(&row.get::<_, String>("id")?)?,
        runtime: RuntimeKind::from_str(&row.get::<_, String>("runtime")?)?,
        role: row.get("role")?,
        workspace: row.get("workspace")?,
        namespace: Namespace::new(row.get::<_, String>("namespace")?)?,
        dir: PathBuf::from(row.get::<_, String>("dir")?),
        state: session_state_from_row(row)?,
        runtime_pid,
        runtime_session: row.get("runtime_session")?,
        transcript_path: row
            .get::<_, Option<String>>("transcript_path")?
            .map(Into::into),
        tmux_pane: row.get("tmux_pane")?,
        agent_config: row.get("agent_config")?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
        started_at: parse_timestamp(&row.get::<_, String>("started_at")?)?,
        terminated_at: parse_optional_timestamp(row.get::<_, Option<String>>("terminated_at")?)?,
        exit_code: optional_i32(row, "exit_code")?,
        updated_at: parse_timestamp(&row.get::<_, String>("updated_at")?)?,
        labels: Vec::new(),
    })
}

fn session_state_from_row(row: &Row<'_>) -> Result<SessionState, SessionRowError> {
    let lost_evidence = row
        .get::<_, Option<String>>("lost_evidence")?
        .as_deref()
        .and_then(lost_evidence_from_sql);
    Ok(SessionState::from_sql(
        &row.get::<_, String>("state")?,
        lost_evidence,
    )?)
}

fn session_lost_evidence(state: SessionState) -> Option<&'static str> {
    match state {
        SessionState::Lost { evidence } => Some(lost_evidence_to_sql(evidence)),
        _ => None,
    }
}

fn optional_i32(row: &Row<'_>, column: &'static str) -> Result<Option<i32>, SessionRowError> {
    row.get::<_, Option<i64>>(column)?
        .map(|value| i32::try_from(value).map_err(|_| integer_out_of_range(column, value)))
        .transpose()
}

fn integer_out_of_range(field: &'static str, value: i64) -> SessionRowError {
    SessionRowError::IntegerOutOfRange { field, value }
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod sessions_tests;
