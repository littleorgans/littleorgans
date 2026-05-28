//! Identity Matters audit storage: `SqliteAuditSink` and the filtered
//! `query_audit` API. Owns the reserved schema fields (`policy_id`,
//! `evaluation_trace`, `denial_reason`) that v2+ policy evaluation can
//! populate without a migration.

pub mod schema;
pub mod sqlite;

use lilo_im_core::AuditRow;
use sqlx::SqlitePool;

pub use sqlite::{AuditFilters, AuditTableColumn, SqliteAuditSink, StoreError};

pub async fn query_audit(
    pool: &SqlitePool,
    filters: AuditFilters,
) -> Result<Vec<AuditRow>, StoreError> {
    SqliteAuditSink::with_pool(pool.clone())
        .query_audit(filters)
        .await
}
