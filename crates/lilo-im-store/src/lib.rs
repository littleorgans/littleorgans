//! Identity Matters audit storage.
//!
//! The default published surface is the backend-neutral, sqlx-free audit
//! contract re-exported from [`lilo_im_core`]: [`AuditSink`], [`AuditRow`], and
//! [`AuditError`]. Consumers query audit data through a concrete sink that
//! implements [`AuditSink`].
//!
//! The concrete SQLite-backed [`AuditStore`] and its filtered `query_audit`
//! API compile only under the `sqlite` feature. `SQLite` is a transition backend;
//! Phase 2 adds a `postgres` feature. Because the default build enables no
//! backend, the published crate carries no concrete `sqlx` type and never
//! depends on the unpublished `lilo-db`. Owns the reserved schema fields
//! (`policy_id`, `evaluation_trace`, `denial_reason`) that v2+ policy
//! evaluation can populate without a migration.

pub mod schema;

#[cfg(feature = "sqlite")]
pub mod sqlite;

pub use lilo_im_core::{AuditError, AuditRow, AuditSink};

#[cfg(feature = "sqlite")]
pub use sqlite::{AuditFilters, AuditStore, AuditTableColumn, StoreError};
