use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use lilo_im_core::{
    Action, AuditDecision, AuditError, AuditRow, AuditSink, Principal, ResourceSpec,
};
use rusqlite::types::ToSql;
use rusqlite::{Connection, Row, params, params_from_iter};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

use crate::config::{audit_db_parent, default_audit_db_path};
use crate::schema::{AUDIT_MIGRATION_SQL, AUDIT_SCHEMA_VERSION, AUDIT_TABLE, SCHEMA_VERSION_TABLE};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("sqlite task join error: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("timestamp parse error: {0}")]
    Timestamp(#[from] chrono::ParseError),
    #[error("uuid parse error: {0}")]
    Uuid(#[from] uuid::Error),
    #[error("audit query limit too large: {0}")]
    LimitTooLarge(usize),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditFilters {
    pub principal: Option<Principal>,
    pub action: Option<Action>,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditTableColumn {
    pub name: String,
    pub data_type: String,
    pub not_null: bool,
    pub primary_key: bool,
}

#[derive(Debug, Clone)]
pub struct SqliteAuditSink {
    path: PathBuf,
}

impl SqliteAuditSink {
    pub async fn new(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::connect(path).await
    }

    pub async fn connect(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = audit_db_parent(&path) {
            std::fs::create_dir_all(parent)?;
        }

        with_connection(path.clone(), |_| Ok(())).await?;

        Ok(Self { path })
    }

    pub async fn connect_default() -> Result<Self, StoreError> {
        Self::connect(default_audit_db_path()).await
    }

    pub async fn run_migrations(&self) -> Result<(), StoreError> {
        with_connection(self.path.clone(), run_audit_migrations).await
    }

    pub async fn query_audit(&self, filters: AuditFilters) -> Result<Vec<AuditRow>, StoreError> {
        with_connection(self.path.clone(), move |connection| {
            query_audit_rows(connection, filters)
        })
        .await
    }

    pub async fn audit_table_columns(&self) -> Result<Vec<AuditTableColumn>, StoreError> {
        with_connection(self.path.clone(), query_audit_table_columns).await
    }

    async fn insert_audit_row(&self, row: AuditRow) -> Result<(), StoreError> {
        with_connection(self.path.clone(), move |connection| {
            insert_audit_row(connection, &row)
        })
        .await
    }
}

impl AuditSink for SqliteAuditSink {
    async fn record(&self, row: AuditRow) -> Result<(), AuditError> {
        self.insert_audit_row(row)
            .await
            .map_err(|error| AuditError::sink(error.to_string()))
    }
}

#[derive(Debug)]
struct AuditRecord {
    id: String,
    timestamp: String,
    principal: String,
    action: String,
    resource: String,
    decision: String,
    session_ref: Option<String>,
    notes: Option<String>,
    policy_id: Option<String>,
    evaluation_trace: Option<String>,
    denial_reason: Option<String>,
}

impl AuditRecord {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            timestamp: row.get("timestamp")?,
            principal: row.get("principal")?,
            action: row.get("action")?,
            resource: row.get("resource")?,
            decision: row.get("decision")?,
            session_ref: row.get("session_ref")?,
            notes: row.get("notes")?,
            policy_id: row.get("policy_id")?,
            evaluation_trace: row.get("evaluation_trace")?,
            denial_reason: row.get("denial_reason")?,
        })
    }

    fn try_into_audit_row(self) -> Result<AuditRow, StoreError> {
        Ok(AuditRow {
            id: Uuid::parse_str(&self.id)?,
            timestamp: DateTime::parse_from_rfc3339(&self.timestamp)?.with_timezone(&Utc),
            principal: serde_json::from_str::<Principal>(&self.principal)?,
            action: serde_json::from_str::<Action>(&self.action)?,
            resource: serde_json::from_str::<ResourceSpec>(&self.resource)?,
            decision: serde_json::from_str::<AuditDecision>(&self.decision)?,
            session_ref: parse_optional_uuid(self.session_ref)?,
            notes: self.notes,
            policy_id: self.policy_id,
            evaluation_trace: self.evaluation_trace,
            denial_reason: self.denial_reason,
        })
    }
}

impl AuditTableColumn {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            name: row.get("name")?,
            data_type: row.get("type")?,
            not_null: row.get::<_, i64>("notnull")? != 0,
            primary_key: row.get::<_, i64>("pk")? != 0,
        })
    }
}

fn serialize_json<T: Serialize>(value: &T) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(Into::into)
}

fn parse_optional_uuid(value: Option<String>) -> Result<Option<Uuid>, StoreError> {
    value
        .map(|id| Uuid::parse_str(&id))
        .transpose()
        .map_err(Into::into)
}

async fn with_connection<T>(
    path: PathBuf,
    operation: impl FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
) -> Result<T, StoreError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut connection = Connection::open(path)?;
        operation(&mut connection)
    })
    .await?
}

fn run_audit_migrations(connection: &mut Connection) -> Result<(), StoreError> {
    connection.execute_batch(&create_schema_version_table_sql())?;
    if schema_version_applied(connection, AUDIT_SCHEMA_VERSION)? {
        return Ok(());
    }

    let transaction = connection.transaction()?;
    transaction.execute_batch(AUDIT_MIGRATION_SQL)?;
    transaction.execute(
        &insert_schema_version_sql(),
        params![AUDIT_SCHEMA_VERSION, Utc::now().to_rfc3339()],
    )?;
    transaction.commit()?;
    Ok(())
}

fn schema_version_applied(connection: &Connection, version: i64) -> Result<bool, StoreError> {
    let applied = connection.query_row(&schema_version_applied_sql(), params![version], |row| {
        row.get::<_, i64>(0)
    })?;
    Ok(applied != 0)
}

fn query_audit_rows(
    connection: &mut Connection,
    filters: AuditFilters,
) -> Result<Vec<AuditRow>, StoreError> {
    let (query, params) = select_audit_sql(filters)?;
    let mut statement = connection.prepare(&query)?;
    let records = statement.query_map(
        params_from_iter(params.iter().map(|param| param.as_ref() as &dyn ToSql)),
        AuditRecord::from_row,
    )?;
    records
        .map(|record| record?.try_into_audit_row())
        .collect::<Result<Vec<_>, _>>()
}

fn query_audit_table_columns(
    connection: &mut Connection,
) -> Result<Vec<AuditTableColumn>, StoreError> {
    let query = audit_table_columns_sql();
    let mut statement = connection.prepare(&query)?;
    let columns = statement.query_map([], AuditTableColumn::from_row)?;
    columns.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn insert_audit_row(connection: &mut Connection, row: &AuditRow) -> Result<(), StoreError> {
    let id = row.id.to_string();
    let timestamp = row.timestamp.to_rfc3339();
    let principal = serialize_json(&row.principal)?;
    let action = serialize_json(&row.action)?;
    let resource = serialize_json(&row.resource)?;
    let decision = serialize_json(&row.decision)?;
    let session_ref = row.session_ref.map(|id| id.to_string());
    let statement = insert_audit_sql();

    connection.execute(
        &statement,
        params![
            id,
            timestamp,
            principal,
            action,
            resource,
            decision,
            session_ref,
            row.notes.as_deref(),
            row.policy_id.as_deref(),
            row.evaluation_trace.as_deref(),
            row.denial_reason.as_deref(),
        ],
    )?;
    Ok(())
}

fn create_schema_version_table_sql() -> String {
    format!(
        "\
CREATE TABLE IF NOT EXISTS {SCHEMA_VERSION_TABLE} (
    version INTEGER NOT NULL PRIMARY KEY,
    applied_at TEXT NOT NULL
)"
    )
}

fn insert_schema_version_sql() -> String {
    format!("INSERT OR IGNORE INTO {SCHEMA_VERSION_TABLE} (version, applied_at) VALUES (?1, ?2)")
}

fn schema_version_applied_sql() -> String {
    format!("SELECT EXISTS(SELECT 1 FROM {SCHEMA_VERSION_TABLE} WHERE version = ?1)")
}

fn select_audit_sql(filters: AuditFilters) -> Result<(String, Vec<Box<dyn ToSql>>), StoreError> {
    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn ToSql>> = Vec::new();

    if let Some(principal) = filters.principal {
        conditions.push("principal = ?");
        params.push(Box::new(serialize_json(&principal)?));
    }
    if let Some(action) = filters.action {
        conditions.push("action = ?");
        params.push(Box::new(serialize_json(&action)?));
    }
    if let Some(since) = filters.since {
        conditions.push("timestamp >= ?");
        params.push(Box::new(since.to_rfc3339()));
    }

    let mut query = format!(
        "\
SELECT id, timestamp, principal, action, resource, decision, session_ref, notes,
       policy_id, evaluation_trace, denial_reason
FROM {AUDIT_TABLE}"
    );
    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }
    query.push_str(" ORDER BY rowid ASC");
    if let Some(limit) = filters.limit {
        let limit = i64::try_from(limit).map_err(|_| StoreError::LimitTooLarge(limit))?;
        query.push_str(" LIMIT ?");
        params.push(Box::new(limit));
    }
    Ok((query, params))
}

fn audit_table_columns_sql() -> String {
    format!("PRAGMA table_info({AUDIT_TABLE})")
}

fn insert_audit_sql() -> String {
    format!(
        "\
INSERT INTO {AUDIT_TABLE} (
    id, timestamp, principal, action, resource, decision, session_ref, notes,
    policy_id, evaluation_trace, denial_reason
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
    )
}
