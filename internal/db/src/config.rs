//! Connection configuration for the Postgres [`LiloDb`](crate::LiloDb) target.

use std::time::Duration;

use anyhow::{Context, Result};

/// Default pool ceiling, matching the prior pool sizing.
const MAX_CONNECTIONS: u32 = 5;
/// Default timeout for acquiring a connection while opening the pool.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Configuration for opening the Postgres database.
#[derive(Clone, Debug)]
pub struct DbConfig {
    /// Postgres connection URL (`postgres://user:pass@host:port/database`).
    pub database_url: String,
    /// Maximum pooled connections.
    pub max_connections: u32,
    /// Timeout for acquiring a connection while opening the pool.
    pub connect_timeout: Duration,
}

impl DbConfig {
    /// Build config from the `LILO_DATABASE_URL` operator contract.
    ///
    /// # Errors
    /// Returns an error when `LILO_DATABASE_URL` is unset or empty.
    pub fn from_env() -> Result<Self> {
        let database_url = lilo_paths::env::database_url()
            .context("LILO_DATABASE_URL is not set; it is the operator database contract")?;
        Ok(Self::from_url(database_url))
    }

    /// Build config from an explicit Postgres URL with default pool sizing.
    pub fn from_url(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            max_connections: MAX_CONNECTIONS,
            connect_timeout: CONNECT_TIMEOUT,
        }
    }
}
