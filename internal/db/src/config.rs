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

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use std::time::Duration;

    use super::DbConfig;

    #[test]
    fn from_url_stores_url_and_default_sizing() {
        let config = DbConfig::from_url("postgres://lilo:lilo@localhost:55432/lilo");
        assert_eq!(
            config.database_url,
            "postgres://lilo:lilo@localhost:55432/lilo"
        );
        assert_eq!(config.max_connections, 5);
        assert_eq!(config.connect_timeout, Duration::from_secs(5));
    }

    #[test]
    fn resolve_prefers_env_over_settings() {
        let home = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            home.path().join("settings.toml"),
            "[database]\nurl = \"postgres://settings/db\"\n",
        )
        .expect("write settings");
        let _guard = EnvGuard::new(&[
            ("LILO_HOME", Some(home.path().to_str().expect("utf-8 path"))),
            ("LILO_DATABASE_URL", Some("postgres://env/db")),
        ]);

        let config = DbConfig::resolve().expect("resolves a url");
        assert_eq!(config.database_url, "postgres://env/db");
    }

    #[test]
    fn resolve_falls_back_to_settings_when_env_unset() {
        let home = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            home.path().join("settings.toml"),
            "[database]\nurl = \"postgres://settings/db\"\n",
        )
        .expect("write settings");
        let _guard = EnvGuard::new(&[
            ("LILO_HOME", Some(home.path().to_str().expect("utf-8 path"))),
            ("LILO_DATABASE_URL", None),
        ]);

        let config = DbConfig::resolve().expect("resolves a url");
        assert_eq!(config.database_url, "postgres://settings/db");
    }

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    /// Serializes process-environment mutation across the resolution tests and
    /// restores the prior values on drop. Mirrors the guard in `lilo-paths`.
    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        originals: Vec<(String, Option<OsString>)>,
    }

    impl EnvGuard {
        fn new(vars: &[(&str, Option<&str>)]) -> Self {
            let lock = ENV_LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .expect("env lock");
            let originals = vars
                .iter()
                .map(|(name, _)| ((*name).to_string(), env::var_os(name)))
                .collect();
            for (name, value) in vars {
                set_env(name, value.map(OsStr::new));
            }
            Self {
                _lock: lock,
                originals,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in &self.originals {
                set_env(name, value.as_deref());
            }
        }
    }

    fn set_env(name: &str, value: Option<&OsStr>) {
        match value {
            Some(value) => {
                // SAFETY: env mutation in these tests is serialized through
                // ENV_LOCK and no thread reads the environment concurrently.
                unsafe { env::set_var(name, value) };
            }
            None => {
                // SAFETY: serialized through ENV_LOCK; no concurrent env readers.
                unsafe { env::remove_var(name) };
            }
        }
    }
}
