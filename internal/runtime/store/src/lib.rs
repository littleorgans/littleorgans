#![forbid(unsafe_code)]

//! Durable `SQLite` lifecycle state for rtmd.
//!
//! This crate owns store configuration, schema modules, and lifecycle
//! persistence while keeping SQL details behind a narrow API.

pub mod config;
pub mod schema;
pub mod sqlite;

pub use config::StoreConfig;
pub use sqlite::LifecycleStore;
