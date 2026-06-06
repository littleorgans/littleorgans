//! Identity Matters audit storage.
//!
//! The default published surface is the backend-neutral, sqlx-free audit
//! contract re-exported from [`lilo_im_core`]: [`AuditSink`], [`AuditRow`], and
//! [`AuditError`]. Consumers query audit data through a concrete sink that
//! implements [`AuditSink`].
//!
//! The concrete Postgres-backed [`AuditStore`] and its filtered `query_audit`
//! API compile only under the `postgres` feature. Because the default build
//! enables no backend, the published crate carries no concrete `sqlx` type and
//! never depends on the unpublished `lilo-db`. Owns the reserved schema fields
//! (`policy_id`, `evaluation_trace`, `denial_reason`) that v2+ policy
//! evaluation can populate without a migration.

pub mod schema;

// Module path stays `sqlite` until the Phase 5 cosmetic rename; the body is
// Postgres.
#[cfg(feature = "postgres")]
pub mod sqlite;

pub use lilo_im_core::{AuditError, AuditRow, AuditSink};

#[cfg(feature = "postgres")]
pub use sqlite::{AuditFilters, AuditStore, AuditTableColumn, StoreError};
