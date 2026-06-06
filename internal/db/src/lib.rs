#![deny(unsafe_code)]

//! Internal database boundary for the composed littleorgans daemons.
//!
//! Phase 1.a introduces the Postgres target ([`LiloDb::open_postgres`],
//! [`LiloDb::pool`], [`DbConfig`], [`test_support::TestDb`]) alongside a
//! quarantined `SQLite` transition surface (see [`transition`]) that keeps the
//! not-yet-migrated typed stores compiling. Phase 2 deletes the transition
//! surface and collapses [`LiloDb`] to a single `PgPool` field.

mod config;
mod transition;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::migrate::Migrator;
use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, Postgres};

pub use config::DbConfig;
pub use transition::{
    ImmediateTx, begin_immediate_pool_tx, begin_immediate_tx, finish_immediate_pool_tx,
    finish_immediate_tx,
};

/// Target Postgres pool handle. Phase 2 end state for every store caller.
pub type LiloPool = sqlx::PgPool;
/// Target Postgres connection type.
pub type LiloConnection = sqlx::PgConnection;
/// Target Postgres transaction type.
pub type LiloTransaction<'a> = sqlx::Transaction<'a, Postgres>;

/// Internal database handle.
///
/// Holds either the target Postgres backing or a transition `SQLite` backing.
/// A single handle is one or the other: every current store caller constructs
/// the `SQLite` backing through [`transition`], and only the Postgres target path
/// constructs [`Backing::Postgres`]. Phase 2 removes the `SQLite` arm.
#[derive(Clone)]
pub struct LiloDb {
    pub(crate) backing: Backing,
}

#[derive(Clone)]
pub(crate) enum Backing {
    /// Target Postgres backing. Phase 2 collapses `LiloDb` to just this pool.
    Postgres(LiloPool),
    /// TRANSITION SCAFFOLDING, removed in Phase 2. `SQLite` backing retained so
    /// not-yet-migrated typed stores keep compiling against the `*_pool`
    /// accessors in [`transition`]. No new code may construct this arm.
    Sqlite(sqlx::SqlitePool),
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

        Ok(Self {
            backing: Backing::Postgres(pool),
        })
    }

    /// Open the target Postgres database from resolved config
    /// (`LILO_DATABASE_URL` over `$LILO_HOME/settings.toml`).
    pub async fn open_postgres_resolved() -> Result<Self> {
        Self::open_postgres(DbConfig::resolve()?).await
    }

    /// Shared Postgres pool accessor (target API).
    ///
    /// # Panics
    /// Panics when called on a transition SQLite-backed handle.
    pub fn pool(&self) -> &LiloPool {
        match &self.backing {
            Backing::Postgres(pool) => pool,
            Backing::Sqlite(_) => {
                panic!(
                    "LiloDb::pool() requires the Postgres backing; this handle is transition SQLite scaffolding"
                )
            }
        }
    }

    /// Acquire a pooled Postgres connection.
    pub async fn acquire(&self) -> Result<PoolConnection<Postgres>> {
        self.pool()
            .acquire()
            .await
            .context("failed to acquire postgres connection")
    }

    /// Begin a Postgres transaction, labelled for error context.
    pub async fn begin(&self, label: &str) -> Result<LiloTransaction<'_>> {
        self.pool()
            .begin()
            .await
            .with_context(|| format!("failed to begin {label}"))
    }

    /// Close the active pool.
    pub async fn close(&self) {
        match &self.backing {
            Backing::Postgres(pool) => pool.close().await,
            Backing::Sqlite(pool) => pool.close().await,
        }
    }
}

/// The Postgres migration set. Single source for the migrations directory; the
/// transition `SQLite` constructors share it (compile scaffolding once the
/// directory holds Postgres SQL).
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
