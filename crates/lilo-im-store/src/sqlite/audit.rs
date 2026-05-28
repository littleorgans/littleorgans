use chrono::{DateTime, Utc};
use lilo_common::sql::WhereClause;
use lilo_im_core::{
    Action, AuditDecision, AuditError, AuditRow, AuditSink, Principal, ResourceSpec,
};
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Executor, QueryBuilder, Row, Sqlite, SqliteConnection, SqlitePool};
use thiserror::Error;
use uuid::Uuid;

use crate::schema::AUDIT_TABLE;

const AUDIT_ROW_COLUMNS: &str = "\
id, timestamp, principal, action, resource, decision, session_ref, notes, policy_id, \
evaluation_trace, denial_reason";
const AUDIT_ROW_PLACEHOLDERS: &str = "?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] sqlx::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("timestamp parse error: {0}")]
    Timestamp(#[from] chrono::ParseError),
    #[error("uuid parse error: {0}")]
    Uuid(#[from] uuid::Error),
    #[error("audit query limit too large: {0}")]
    LimitTooLarge(usize),
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
    pool: SqlitePool,
}

impl SqliteAuditSink {
    #[must_use]
    pub fn with_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn query_audit(&self, filters: AuditFilters) -> Result<Vec<AuditRow>, StoreError> {
        query_audit_rows(&self.pool, filters).await
    }

    pub async fn audit_table_columns(&self) -> Result<Vec<AuditTableColumn>, StoreError> {
        let sql = format!("PRAGMA table_info({AUDIT_TABLE})");
        let rows = sqlx::query(&sql).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(AuditTableColumn {
                    name: row.try_get("name")?,
                    data_type: row.try_get("type")?,
                    not_null: row.try_get::<i64, _>("notnull")? != 0,
                    primary_key: row.try_get::<i64, _>("pk")? != 0,
                })
            })
            .collect()
    }

    async fn insert_audit_row(&self, row: AuditRow) -> Result<(), StoreError> {
        insert_audit_row(&self.pool, &row).await
    }
}

impl AuditSink for SqliteAuditSink {
    async fn record(&self, row: AuditRow) -> Result<(), AuditError> {
        self.insert_audit_row(row)
            .await
            .map_err(|error| AuditError::sink(error.to_string()))
    }
}

pub async fn record_audit_in_tx(
    conn: &mut SqliteConnection,
    row: &AuditRow,
) -> Result<(), StoreError> {
    insert_audit_row_with(conn, row).await
}

#[derive(Debug)]
struct EncodedAuditRow {
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

impl EncodedAuditRow {
    fn from_audit_row(row: &AuditRow) -> Result<Self, StoreError> {
        Ok(Self {
            id: row.id.to_string(),
            timestamp: row.timestamp.to_rfc3339(),
            principal: serialize_json(&row.principal)?,
            action: serialize_json(&row.action)?,
            resource: serialize_json(&row.resource)?,
            decision: serialize_json(&row.decision)?,
            session_ref: row
                .session_ref
                .as_ref()
                .map(std::string::ToString::to_string),
            notes: row.notes.clone(),
            policy_id: row.policy_id.clone(),
            evaluation_trace: row.evaluation_trace.clone(),
            denial_reason: row.denial_reason.clone(),
        })
    }

    fn from_row(row: &SqliteRow) -> Result<Self, StoreError> {
        Ok(Self {
            id: row.try_get("id")?,
            timestamp: row.try_get("timestamp")?,
            principal: row.try_get("principal")?,
            action: row.try_get("action")?,
            resource: row.try_get("resource")?,
            decision: row.try_get("decision")?,
            session_ref: row.try_get("session_ref")?,
            notes: row.try_get("notes")?,
            policy_id: row.try_get("policy_id")?,
            evaluation_trace: row.try_get("evaluation_trace")?,
            denial_reason: row.try_get("denial_reason")?,
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

fn serialize_json<T: Serialize>(value: &T) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(Into::into)
}

fn parse_optional_uuid(value: Option<String>) -> Result<Option<Uuid>, StoreError> {
    value
        .map(|id| Uuid::parse_str(&id))
        .transpose()
        .map_err(Into::into)
}

async fn query_audit_rows(
    pool: &SqlitePool,
    filters: AuditFilters,
) -> Result<Vec<AuditRow>, StoreError> {
    let mut query = QueryBuilder::<Sqlite>::new(format!(
        "\
SELECT {AUDIT_ROW_COLUMNS}
FROM {AUDIT_TABLE}",
    ));
    let mut where_clause = WhereClause::new();
    if let Some(principal) = filters.principal {
        query.push(where_clause.predicate_prefix());
        query
            .push("principal = ")
            .push_bind(serialize_json(&principal)?);
    }
    if let Some(action) = filters.action {
        query.push(where_clause.predicate_prefix());
        query.push("action = ").push_bind(serialize_json(&action)?);
    }
    if let Some(since) = filters.since {
        query.push(where_clause.predicate_prefix());
        query.push("timestamp >= ").push_bind(since.to_rfc3339());
    }
    query.push(" ORDER BY rowid ASC");
    if let Some(limit) = filters.limit {
        let limit = i64::try_from(limit).map_err(|_| StoreError::LimitTooLarge(limit))?;
        query.push(" LIMIT ").push_bind(limit);
    }

    let rows = query.build().fetch_all(pool).await?;
    rows.into_iter()
        .map(|row| EncodedAuditRow::from_row(&row))
        .map(|record| record.and_then(EncodedAuditRow::try_into_audit_row))
        .collect()
}

async fn insert_audit_row(pool: &SqlitePool, row: &AuditRow) -> Result<(), StoreError> {
    insert_audit_row_with(pool, row).await
}

async fn insert_audit_row_with<'e, E>(executor: E, row: &AuditRow) -> Result<(), StoreError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let encoded = EncodedAuditRow::from_audit_row(row)?;
    let sql = format!(
        "INSERT INTO {AUDIT_TABLE} ({AUDIT_ROW_COLUMNS}) VALUES ({AUDIT_ROW_PLACEHOLDERS})",
    );

    sqlx::query(&sql)
        .bind(encoded.id)
        .bind(encoded.timestamp)
        .bind(encoded.principal)
        .bind(encoded.action)
        .bind(encoded.resource)
        .bind(encoded.decision)
        .bind(encoded.session_ref)
        .bind(encoded.notes)
        .bind(encoded.policy_id)
        .bind(encoded.evaluation_trace)
        .bind(encoded.denial_reason)
        .execute(executor)
        .await?;
    Ok(())
}
