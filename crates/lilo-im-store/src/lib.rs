//! Identity Matters audit storage: `SqliteAuditSink` and the filtered
//! `query_audit` API. Owns the reserved schema fields (`policy_id`,
//! `evaluation_trace`, `denial_reason`) that v2+ policy evaluation can
//! populate without a migration.

pub mod config;
pub mod schema;
pub mod sqlite;

use std::path::Path;

use lilo_im_core::AuditRow;

pub use config::default_audit_db_path;
pub use sqlite::{AuditFilters, AuditTableColumn, SqliteAuditSink, StoreError};

pub async fn query_audit(
    path: impl AsRef<Path>,
    filters: AuditFilters,
) -> Result<Vec<AuditRow>, StoreError> {
    SqliteAuditSink::connect(path)
        .await?
        .query_audit(filters)
        .await
}

pub async fn query_default_audit() -> Result<Vec<AuditRow>, StoreError> {
    query_audit(default_audit_db_path(), AuditFilters::default()).await
}

pub async fn query_default_audit_filtered(
    filters: AuditFilters,
) -> Result<Vec<AuditRow>, StoreError> {
    query_audit(default_audit_db_path(), filters).await
}
