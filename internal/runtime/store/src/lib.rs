#![forbid(unsafe_code)]

//! Durable Postgres lifecycle state for rtmd.
//!
//! This crate owns schema modules and lifecycle persistence while keeping SQL
//! details behind a narrow API.

pub mod postgres;
pub mod schema;

pub use postgres::LifecycleStore;
