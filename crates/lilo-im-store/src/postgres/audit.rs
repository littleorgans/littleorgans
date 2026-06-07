use chrono::{DateTime, Utc};
use lilo_common::id::SessionId;
use lilo_common::sql::WhereClause;
use lilo_im_core::{
    Action, AuditDecision, AuditError, AuditRow, AuditSink, Principal, ResourceSpec,
};
use serde::Serialize;
use sqlx::postgres::PgRow;
use sqlx::{Executor, PgConnection, PgPool, Postgres, QueryBuilder, Row};
use thiserror::Error;

use crate::schema::AUDIT_TABLE;

const AUDIT_ROW_COLUMNS: &str = "\
id, timestamp, principal, action, resource, decision, session_ref, notes, policy_id, \
evaluation_trace, denial_reason";
const AUDIT_ROW_PLACEHOLDERS: &str = "$1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
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
pub struct AuditStore {
    pool: PgPool,
}

impl AuditStore {
    #[must_use]
    pub fn with_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn query_audit(&self, filters: AuditFilters) -> Result<Vec<AuditRow>, StoreError> {
        query_audit_rows(&self.pool, filters).await
    }

    pub async fn audit_table_columns(&self) -> Result<Vec<AuditTableColumn>, StoreError> {
        let rows = sqlx::query(
            r"
            SELECT c.column_name AS name,
                   c.data_type AS data_type,
                   (c.is_nullable = 'NO') AS not_null,
                   COALESCE(pk.is_pk, false) AS primary_key
            FROM information_schema.columns c
            LEFT JOIN (
                SELECT kcu.column_name, true AS is_pk
                FROM information_schema.table_constraints tc
                JOIN information_schema.key_column_usage kcu
                  ON kcu.constraint_name = tc.constraint_name
                 AND kcu.table_schema = tc.table_schema
                WHERE tc.table_name = $1
                  AND tc.table_schema = current_schema()
                  AND tc.constraint_type = 'PRIMARY KEY'
            ) pk ON pk.column_name = c.column_name
            WHERE c.table_name = $1
              AND c.table_schema = current_schema()
            ORDER BY c.ordinal_position
            ",
        )
        .bind(AUDIT_TABLE)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(AuditTableColumn {
                    name: row.try_get("name")?,
                    data_type: row.try_get("data_type")?,
                    not_null: row.try_get("not_null")?,
                    primary_key: row.try_get("primary_key")?,
                })
            })
            .collect()
    }

    async fn insert_audit_row(&self, row: AuditRow) -> Result<(), StoreError> {
        insert_audit_row(&self.pool, &row).await
    }
}

impl AuditSink for AuditStore {
    async fn record(&self, row: AuditRow) -> Result<(), AuditError> {
        self.insert_audit_row(row)
            .await
            .map_err(|error| AuditError::sink(error.to_string()))
    }
}

pub async fn record_audit_in_tx(conn: &mut PgConnection, row: &AuditRow) -> Result<(), StoreError> {
    insert_audit_row_with(conn, row).await
}

#[derive(Debug)]
struct EncodedAuditRow {
    id: String,
    timestamp: DateTime<Utc>,
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
            timestamp: row.timestamp,
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

    fn from_row(row: &PgRow) -> Result<Self, StoreError> {
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
            id: self.id.parse()?,
            timestamp: self.timestamp,
            principal: serde_json::from_str::<Principal>(&self.principal)?,
            action: serde_json::from_str::<Action>(&self.action)?,
            resource: serde_json::from_str::<ResourceSpec>(&self.resource)?,
            decision: serde_json::from_str::<AuditDecision>(&self.decision)?,
            session_ref: parse_optional_session_id(self.session_ref)?,
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

fn parse_optional_session_id(value: Option<String>) -> Result<Option<SessionId>, StoreError> {
    value.map(|id| id.parse()).transpose().map_err(Into::into)
}

async fn query_audit_rows(
    pool: &PgPool,
    filters: AuditFilters,
) -> Result<Vec<AuditRow>, StoreError> {
    let mut query = QueryBuilder::<Postgres>::new(format!(
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
        query.push("timestamp >= ").push_bind(since);
    }
    // `seq` is a monotonic identity column: insertion order, stable even when
    // two rows share a `timestamp` (Postgres has no implicit row order to fall back on).
    query.push(" ORDER BY seq ASC");
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

async fn insert_audit_row(pool: &PgPool, row: &AuditRow) -> Result<(), StoreError> {
    insert_audit_row_with(pool, row).await
}

async fn insert_audit_row_with<'e, E>(executor: E, row: &AuditRow) -> Result<(), StoreError>
where
    E: Executor<'e, Database = Postgres>,
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
