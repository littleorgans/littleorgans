#![forbid(unsafe_code)]

//! Durable Postgres lifecycle state for rtmd.
//!
//! This crate owns store configuration, schema modules, and lifecycle
//! persistence while keeping SQL details behind a narrow API.

pub mod config;
pub mod postgres;
pub mod schema;

pub use config::StoreConfig;
pub use postgres::LifecycleStore;
