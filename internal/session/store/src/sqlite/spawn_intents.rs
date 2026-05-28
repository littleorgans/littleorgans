use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};
use lilo_rm_core::{
    Lifecycle, LifecycleState, LogAvailability, SpawnRequest as RuntimeSpawnRequest,
};
use lilo_session_core::{Label, Namespace, RuntimeKind, Session, SessionState};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqliteConnection};
use thiserror::Error;
use uuid::Uuid;

use super::SqliteStore;

#[derive(Debug, Error)]
pub enum SpawnIntentError {
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Uuid(#[from] uuid::Error),
    #[error(transparent)]
    Namespace(#[from] lilo_session_core::NamespaceError),
    #[error("unknown spawn intent status: {0}")]
    UnknownStatus(String),
    #[error("running lifecycle missing runtime pid for session {0}")]
    MissingRuntimePid(Uuid),
    #[error("running lifecycle expected for session {0}")]
    NotRunning(Uuid),
    #[error("timestamp out of range: {0}")]
    TimestampOutOfRange(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnIntentStatus {
    Pending,
    Resolved,
    Aborted,
}

const STATUS_PENDING: &str = "pending";
const STATUS_RESOLVED: &str = "resolved";
const STATUS_ABORTED: &str = "aborted";

impl SpawnIntentStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => STATUS_PENDING,
            Self::Resolved => STATUS_RESOLVED,
            Self::Aborted => STATUS_ABORTED,
        }
    }

    fn parse(value: String) -> Result<Self, SpawnIntentError> {
        match value.as_str() {
            STATUS_PENDING => Ok(Self::Pending),
            STATUS_RESOLVED => Ok(Self::Resolved),
            STATUS_ABORTED => Ok(Self::Aborted),
            _ => Err(SpawnIntentError::UnknownStatus(value)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionDraft {
    pub id: Uuid,
    pub runtime: RuntimeKind,
    pub role: String,
    pub workspace: String,
    pub namespace: Namespace,
    pub dir: PathBuf,
    pub labels: Vec<Label>,
    pub agent_config: Option<String>,
    pub created_at_ms: i64,
}

impl SessionDraft {
    #[must_use]
    pub fn new(session: &Session) -> Self {
        Self {
            id: session.id,
            runtime: session.runtime,
            role: session.role.clone(),
            workspace: session.workspace.clone(),
            namespace: session.namespace.clone(),
            dir: session.dir.clone(),
            labels: session.labels.clone(),
            agent_config: session.agent_config.clone(),
            created_at_ms: session.created_at.timestamp_millis(),
        }
    }

    pub fn running_session(
        &self,
        lifecycle: &Lifecycle,
        stdout_path: Option<PathBuf>,
        updated_at: DateTime<Utc>,
    ) -> Result<Session, SpawnIntentError> {
        if lifecycle.state != LifecycleState::Running {
            return Err(SpawnIntentError::NotRunning(self.id));
        }
        let runtime_pid = lifecycle
            .runtime_pid
            .ok_or(SpawnIntentError::MissingRuntimePid(self.id))?;
        Ok(Session {
            id: self.id,
            runtime: self.runtime,
            role: self.role.clone(),
            workspace: self.workspace.clone(),
            namespace: self.namespace.clone(),
            dir: self.dir.clone(),
            labels: self.labels.clone(),
            state: SessionState::Running,
            runtime_pid,
            runtime_session: None,
            transcript_path: stdout_path.or_else(|| lifecycle_transcript_path(lifecycle)),
            tmux_pane: lifecycle.tmux_pane.as_ref().map(ToString::to_string),
            agent_config: self.agent_config.clone(),
            created_at: timestamp_millis(self.created_at_ms)?,
            started_at: lifecycle.start_time.unwrap_or(updated_at),
            terminated_at: None,
            exit_code: None,
            updated_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSpawnIntent {
    pub session_id: Uuid,
    pub operation_id: String,
    pub spawn_request: RuntimeSpawnRequest,
    pub session_draft: SessionDraft,
    pub created_at: i64,
}

impl PendingSpawnIntent {
    #[must_use]
    pub fn new(
        operation_id: Uuid,
        spawn_request: RuntimeSpawnRequest,
        session_draft: SessionDraft,
    ) -> Self {
        Self {
            session_id: session_draft.id,
            operation_id: operation_id.to_string(),
            spawn_request,
            session_draft,
            created_at: Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSpawnIntent {
    pub session_id: Uuid,
    pub operation_id: String,
    pub status: SpawnIntentStatus,
    pub spawn_request: RuntimeSpawnRequest,
    pub session_draft: SessionDraft,
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
    pub aborted_reason: Option<String>,
}

enum SpawnIntentStatusUpdate<'a> {
    Resolved,
    Aborted { reason: &'a str },
}

impl SpawnIntentStatusUpdate<'_> {
    const fn status(&self) -> SpawnIntentStatus {
        match self {
            Self::Resolved => SpawnIntentStatus::Resolved,
            Self::Aborted { .. } => SpawnIntentStatus::Aborted,
        }
    }

    const fn resolved_at(&self, now_ms: i64) -> Option<i64> {
        match self {
            Self::Resolved => Some(now_ms),
            Self::Aborted { .. } => None,
        }
    }

    fn aborted_reason(&self) -> Option<&str> {
        match self {
            Self::Resolved => None,
            Self::Aborted { reason } => Some(reason),
        }
    }
}

impl SqliteStore {
    pub async fn insert_pending_spawn_intent(
        &self,
        intent: &PendingSpawnIntent,
    ) -> Result<(), SpawnIntentError> {
        insert_pending_spawn_intent_with(&self.pool, intent).await
    }

    pub async fn insert_pending_spawn_intent_in(
        &self,
        conn: &mut SqliteConnection,
        intent: &PendingSpawnIntent,
    ) -> Result<(), SpawnIntentError> {
        insert_pending_spawn_intent_with(conn, intent).await
    }

    pub async fn resolve_spawn_intent(&self, session_id: Uuid) -> Result<(), SpawnIntentError> {
        resolve_spawn_intent_with(&self.pool, session_id, Utc::now().timestamp_millis()).await
    }

    pub async fn resolve_spawn_intent_in(
        &self,
        conn: &mut SqliteConnection,
        session_id: Uuid,
    ) -> Result<(), SpawnIntentError> {
        resolve_spawn_intent_with(conn, session_id, Utc::now().timestamp_millis()).await
    }

    pub async fn abort_spawn_intent(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), SpawnIntentError> {
        abort_spawn_intent_with(
            &self.pool,
            session_id,
            reason,
            Utc::now().timestamp_millis(),
        )
        .await
    }

    pub async fn abort_spawn_intent_in(
        &self,
        conn: &mut SqliteConnection,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), SpawnIntentError> {
        abort_spawn_intent_with(conn, session_id, reason, Utc::now().timestamp_millis()).await
    }

    pub async fn list_pending_spawn_intents(
        &self,
    ) -> Result<Vec<SessionSpawnIntent>, SpawnIntentError> {
        let rows = sqlx::query(
            "SELECT session_id, operation_id, status, spawn_request_json, session_draft_json,
                    created_at, updated_at, resolved_at, aborted_reason
             FROM session_spawn_intents
             WHERE status = ?
             ORDER BY created_at",
        )
        .bind(SpawnIntentStatus::Pending.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(intent_from_row).collect()
    }
}

async fn insert_pending_spawn_intent_with<'e, E>(
    executor: E,
    intent: &PendingSpawnIntent,
) -> Result<(), SpawnIntentError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let spawn_request_json = serde_json::to_string(&intent.spawn_request)?;
    let session_draft_json = serde_json::to_string(&intent.session_draft)?;
    sqlx::query(
        "INSERT INTO session_spawn_intents
            (session_id, operation_id, status, spawn_request_json, session_draft_json,
             created_at, updated_at, resolved_at, aborted_reason)
         VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL)",
    )
    .bind(intent.session_id.to_string())
    .bind(&intent.operation_id)
    .bind(SpawnIntentStatus::Pending.as_str())
    .bind(spawn_request_json)
    .bind(session_draft_json)
    .bind(intent.created_at)
    .bind(intent.created_at)
    .execute(executor)
    .await?;
    Ok(())
}

async fn resolve_spawn_intent_with<'e, E>(
    executor: E,
    session_id: Uuid,
    now_ms: i64,
) -> Result<(), SpawnIntentError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    update_spawn_intent_status_with(
        executor,
        session_id,
        now_ms,
        SpawnIntentStatusUpdate::Resolved,
    )
    .await
}

async fn abort_spawn_intent_with<'e, E>(
    executor: E,
    session_id: Uuid,
    reason: &str,
    now_ms: i64,
) -> Result<(), SpawnIntentError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    update_spawn_intent_status_with(
        executor,
        session_id,
        now_ms,
        SpawnIntentStatusUpdate::Aborted { reason },
    )
    .await
}

async fn update_spawn_intent_status_with<'e, E>(
    executor: E,
    session_id: Uuid,
    now_ms: i64,
    update: SpawnIntentStatusUpdate<'_>,
) -> Result<(), SpawnIntentError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let aborted_reason = update.aborted_reason().map(str::to_owned);
    sqlx::query(
        "UPDATE session_spawn_intents
         SET status = ?, updated_at = ?, resolved_at = ?, aborted_reason = ?
         WHERE session_id = ?",
    )
    .bind(update.status().as_str())
    .bind(now_ms)
    .bind(update.resolved_at(now_ms))
    .bind(aborted_reason)
    .bind(session_id.to_string())
    .execute(executor)
    .await?;
    Ok(())
}

fn intent_from_row(row: &SqliteRow) -> Result<SessionSpawnIntent, SpawnIntentError> {
    Ok(SessionSpawnIntent {
        session_id: Uuid::parse_str(&row.try_get::<String, _>("session_id")?)?,
        operation_id: row.try_get("operation_id")?,
        status: SpawnIntentStatus::parse(row.try_get("status")?)?,
        spawn_request: serde_json::from_str(&row.try_get::<String, _>("spawn_request_json")?)?,
        session_draft: serde_json::from_str(&row.try_get::<String, _>("session_draft_json")?)?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        resolved_at: row.try_get("resolved_at")?,
        aborted_reason: row.try_get("aborted_reason")?,
    })
}

fn lifecycle_transcript_path(lifecycle: &Lifecycle) -> Option<PathBuf> {
    match lifecycle.log_availability.as_ref() {
        Some(LogAvailability::Headless { stdout_path, .. }) => Some(stdout_path.clone()),
        Some(LogAvailability::TmuxPaneSnapshot | LogAvailability::Unavailable { .. }) | None => {
            None
        }
    }
}

fn timestamp_millis(value: i64) -> Result<DateTime<Utc>, SpawnIntentError> {
    Utc.timestamp_millis_opt(value)
        .single()
        .ok_or(SpawnIntentError::TimestampOutOfRange(value))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use lilo_rm_core::{
        HeadlessSpawnTarget, IsolationPolicy, RuntimeKind as RuntimeRuntimeKind, SpawnTarget,
    };
    use lilo_session_core::{Namespace, RuntimeKind};

    use super::*;

    #[tokio::test]
    async fn intent_repository_inserts_and_lists_pending() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let intent = test_intent();

        store
            .insert_pending_spawn_intent(&intent)
            .await
            .expect("insert pending intent");

        let pending = store
            .list_pending_spawn_intents()
            .await
            .expect("list pending intents");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].session_id, intent.session_id);
        assert_eq!(pending[0].status, SpawnIntentStatus::Pending);
        assert_eq!(pending[0].spawn_request, intent.spawn_request);
        assert_eq!(pending[0].session_draft, intent.session_draft);
    }

    #[tokio::test]
    async fn intent_repository_resolves_pending_intent() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let intent = test_intent();
        store
            .insert_pending_spawn_intent(&intent)
            .await
            .expect("insert pending intent");

        store
            .resolve_spawn_intent(intent.session_id)
            .await
            .expect("resolve intent");

        let pending = store
            .list_pending_spawn_intents()
            .await
            .expect("list pending intents");
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn intent_repository_aborts_pending_intent() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let intent = test_intent();
        store
            .insert_pending_spawn_intent(&intent)
            .await
            .expect("insert pending intent");

        store
            .abort_spawn_intent(intent.session_id, "runtime spawn failed")
            .await
            .expect("abort intent");

        let pending = store
            .list_pending_spawn_intents()
            .await
            .expect("list pending intents");
        assert!(pending.is_empty());
    }

    fn test_intent() -> PendingSpawnIntent {
        let id = Uuid::now_v7();
        let now = Utc::now();
        let session = Session {
            id,
            runtime: RuntimeKind::Claude,
            role: "worker".to_owned(),
            workspace: "default".to_owned(),
            namespace: Namespace::default(),
            dir: PathBuf::from("/tmp"),
            labels: Vec::new(),
            state: SessionState::Running,
            runtime_pid: 1,
            runtime_session: None,
            transcript_path: None,
            tmux_pane: None,
            agent_config: None,
            created_at: now,
            started_at: now,
            terminated_at: None,
            exit_code: None,
            updated_at: now,
        };
        PendingSpawnIntent::new(
            Uuid::now_v7(),
            RuntimeSpawnRequest {
                session_id: id,
                runtime: RuntimeRuntimeKind::Claude,
                isolation: IsolationPolicy::Host,
                image: None,
                env: Vec::new(),
                mounts: Vec::new(),
                cwd: PathBuf::from("/tmp"),
                target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
                force: false,
                shell_resume: None,
            },
            SessionDraft::new(&session),
        )
    }
}
