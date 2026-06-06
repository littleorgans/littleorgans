//! Connection configuration for the Postgres [`LiloDb`](crate::LiloDb) target.

use std::time::Duration;

use anyhow::{Context, Result};
use lilo_paths::Settings;
use lilo_paths::env::resolve_database_url;

/// Default pool ceiling, matching the prior pool sizing.
const MAX_CONNECTIONS: u32 = 5;
/// Default timeout for acquiring a connection while opening the pool.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Actionable guidance when no database URL is configured anywhere.
const DATABASE_URL_GUIDANCE: &str = "no Postgres database URL configured: set LILO_DATABASE_URL, or add `[database] url` to $LILO_HOME/settings.toml (copy settings.example.toml). Refusing to connect to a guessed host.";

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
    /// Resolve config from `LILO_DATABASE_URL` over `$LILO_HOME/settings.toml`.
    ///
    /// # Errors
    /// Returns an error when `settings.toml` is present but malformed, or when
    /// no database URL is configured by env or settings.
    pub fn resolve() -> Result<Self> {
        let settings = Settings::load().context("failed to load settings.toml")?;
        let database_url = resolve_database_url(&settings).context(DATABASE_URL_GUIDANCE)?;
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
