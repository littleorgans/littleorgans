#![deny(unsafe_code)]

//! Internal database boundary for the composed littleorgans daemons.
//!
//! [`LiloDb`] owns a single Postgres [`PgPool`]: connection, migration, pool
//! lifecycle, transactions ([`LiloDb::begin`] / [`commit_or_rollback`]), and a
//! test fixture ([`test_support::TestDb`]).

mod config;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::migrate::Migrator;
use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, Postgres};

pub use config::DbConfig;

/// Target Postgres pool handle. Phase 2 end state for every store caller.
pub type LiloPool = sqlx::PgPool;
/// Target Postgres connection type.
pub type LiloConnection = sqlx::PgConnection;
/// Target Postgres transaction type.
pub type LiloTransaction<'a> = sqlx::Transaction<'a, Postgres>;

/// Internal database handle: a single shared Postgres pool.
#[derive(Clone)]
pub struct LiloDb {
    pool: LiloPool,
}

impl LiloDb {
    /// Open the target Postgres database: connect, bound the pool, migrate.
    ///
    /// Connection and migration errors carry a `host:port/database` descriptor
    /// so the failed target is identifiable without leaking the password.
    pub async fn open_postgres(config: DbConfig) -> Result<Self> {
        let options = PgConnectOptions::from_str(&config.database_url)
            .with_context(|| format!("invalid postgres url {}", redacted(&config.database_url)))?
            .disable_statement_logging();
        let descriptor = describe(&options);

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(config.connect_timeout)
            .connect_with(options)
            .await
            .with_context(|| format!("failed to connect to postgres {descriptor}"))?;

        migrator()
            .run(&pool)
            .await
            .with_context(|| format!("failed to run postgres migrations on {descriptor}"))?;

        Ok(Self { pool })
    }

    /// Open the target Postgres database from resolved config
    /// (`LILO_DATABASE_URL` over `$LILO_HOME/settings.toml`).
    pub async fn open_postgres_resolved() -> Result<Self> {
        Self::open_postgres(DbConfig::resolve()?).await
    }

    /// Shared Postgres pool accessor.
    pub fn pool(&self) -> &LiloPool {
        &self.pool
    }

    /// Acquire a pooled Postgres connection.
    pub async fn acquire(&self) -> Result<PoolConnection<Postgres>> {
        self.pool
            .acquire()
            .await
            .context("failed to acquire postgres connection")
    }

    /// Begin a Postgres transaction, labelled for error context.
    pub async fn begin(&self, label: &str) -> Result<LiloTransaction<'_>> {
        self.pool
            .begin()
            .await
            .with_context(|| format!("failed to begin {label}"))
    }

    /// Close the active pool.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// Commit `tx` when `result` is `Ok`, otherwise drop it (sqlx rolls back on
/// drop) and return the error. Threads a single pool-scoped transaction across
/// store crates without naming any locking behavior.
///
/// # Errors
/// Returns the original error on the `Err` path, or a commit failure (mapped
/// through `E: From<sqlx::Error>`) on the `Ok` path.
pub async fn commit_or_rollback<T, E>(
    tx: LiloTransaction<'_>,
    result: std::result::Result<T, E>,
) -> std::result::Result<T, E>
where
    E: From<sqlx::Error>,
{
    match result {
        Ok(value) => {
            tx.commit().await.map_err(E::from)?;
            Ok(value)
        }
        Err(error) => {
            let _ = tx.rollback().await;
            Err(error)
        }
    }
}

/// The Postgres migration set (`internal/db/migrations`).
pub(crate) fn migrator() -> Migrator {
    sqlx::migrate!("./migrations")
}

/// `host:port/database` descriptor for error context; never includes the password.
fn describe(options: &PgConnectOptions) -> String {
    format!(
        "{}:{}/{}",
        options.get_host(),
        options.get_port(),
        options.get_database().unwrap_or("<unknown>")
    )
}

/// Redact userinfo (`user:pass@`) from a URL so a parse error never echoes a
/// password. Best effort: used only when the URL fails to parse.
pub(crate) fn redacted(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let authority_start = scheme_end + 3;
    match url[authority_start..].find('@') {
        Some(offset) => {
            let at = authority_start + offset;
            format!("{}***@{}", &url[..authority_start], &url[at + 1..])
        }
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use anyhow::Result;
    use sqlx::Connection;

    use crate::test_support::{TestDb, admin_url, database_name_of};

    const EXPECTED_TABLES: [&str; 10] = [
        "identity_audit",
        "message_deliveries",
        "messages",
        "runtime_lifecycle",
        "runtime_metadata",
        "session_event_cursor",
        "session_labels",
        "session_namespaces",
        "session_sessions",
        "session_spawn_intents",
    ];

    // The DB-backed tests below are #[ignore]d so the default suite (and
    // `moon ci`) reports them honestly as skipped when no Postgres is
    // configured, rather than silently passing. Run them with
    // `just test-db` (or `cargo nextest run -p lilo-db --run-ignored all`)
    // after setting LILO_TEST_DATABASE_URL or copying settings.example.toml;
    // with no database, TestDb::create()'s loud error is the honest failure.

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL or copy settings.example.toml; run with --run-ignored all"]
    async fn open_postgres_runs_migrations_and_creates_unified_schema() -> Result<()> {
        let fixture = TestDb::create().await?;
        let tables: Vec<String> = sqlx::query_scalar(
            r"
            SELECT tablename
            FROM pg_catalog.pg_tables
            WHERE schemaname = 'public'
              AND tablename NOT LIKE '\_%'
            ORDER BY tablename
            ",
        )
        .fetch_all(fixture.db().pool())
        .await?;

        let expected: BTreeSet<String> = EXPECTED_TABLES.iter().map(ToString::to_string).collect();
        assert_eq!(tables.into_iter().collect::<BTreeSet<_>>(), expected);
        fixture.cleanup().await
    }

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL or copy settings.example.toml; run with --run-ignored all"]
    async fn open_postgres_yields_a_live_connection() -> Result<()> {
        let fixture = TestDb::create().await?;
        let value: i32 = sqlx::query_scalar("SELECT 1")
            .fetch_one(fixture.db().pool())
            .await?;
        assert_eq!(value, 1);
        fixture.cleanup().await
    }

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL or copy settings.example.toml; run with --run-ignored all"]
    async fn cleanup_drops_the_created_database() -> Result<()> {
        let fixture = TestDb::create().await?;
        let name = database_name_of(fixture.database_url())?;
        fixture.cleanup().await?;

        let mut admin = sqlx::PgConnection::connect(&admin_url()?).await?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
                .bind(&name)
                .fetch_one(&mut admin)
                .await?;
        admin.close().await?;

        assert!(!exists, "cleanup must drop the test database {name}");
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL or copy settings.example.toml; run with --run-ignored all"]
    async fn parallel_fixtures_do_not_collide() -> Result<()> {
        let handles: Vec<_> = (0..4).map(|_| tokio::spawn(TestDb::create())).collect();
        let mut fixtures = Vec::with_capacity(handles.len());
        for handle in handles {
            fixtures.push(handle.await??);
        }

        let names: BTreeSet<String> = fixtures
            .iter()
            .map(|f| f.database_url().to_string())
            .collect();
        assert_eq!(
            names.len(),
            fixtures.len(),
            "test database urls must be unique"
        );

        for fixture in &fixtures {
            let value: i32 = sqlx::query_scalar("SELECT 1")
                .fetch_one(fixture.db().pool())
                .await?;
            assert_eq!(value, 1);
        }

        for fixture in fixtures {
            fixture.cleanup().await?;
        }
        Ok(())
    }
}
