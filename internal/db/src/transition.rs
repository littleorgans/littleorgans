//! TRANSITION SCAFFOLDING (removed in Phase 2).
//!
//! Everything in this module is SQLite-shaped infrastructure retained only so
//! the not-yet-migrated typed stores and daemon callers keep compiling while
//! query migration is deferred to Phase 1.b/2. It is deliberately quarantined
//! here so it cannot be confused with the Postgres target API in the crate
//! root. No new code may use these symbols, and no replacement `SQLite` aliases
//! may be added.
//!
//! These constructors run their OWN quarantined `SQLite` migration directory
//! ([`internal/db/migrations-sqlite`]) so the live daemon and the not-yet-
//! migrated stores keep working on `SQLite` while `internal/db/migrations` holds
//! the Postgres target schema. The two dirs run green in parallel; the `SQLite`
//! one is deleted in Phase 2 when the last caller leaves the transition surface.

use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use lilo_paths::LiloPaths;
use sqlx::migrate::Migrator;
use sqlx::pool::PoolConnection;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Database, Sqlite, SqliteConnection, SqlitePool, TransactionManager};

use crate::{Backing, LiloDb};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: u32 = 5;
const WAL_AUTOCHECKPOINT_PAGES: &str = "1000";

/// The quarantined `SQLite` migration set for the transition backing. Separate
/// from [`crate::migrator`] (the Postgres target) so each backend runs its own
/// dialect against its own pool.
fn sqlite_migrator() -> Migrator {
    sqlx::migrate!("./migrations-sqlite")
}

/// Transition `SQLite` constructors and pool accessors.
impl LiloDb {
    /// Transition: open the `SQLite` store at the operator paths' db path.
    pub async fn open(paths: &LiloPaths) -> Result<Self> {
        Self::open_path(paths.db_path()).await
    }

    /// Transition: open (or create) the `SQLite` store at `path` and migrate it.
    pub async fn open_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!("failed to create lilo db directory {}", parent.display())
            })?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(BUSY_TIMEOUT)
            .pragma("wal_autocheckpoint", WAL_AUTOCHECKPOINT_PAGES);
        let pool = SqlitePoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .connect_with(options)
            .await
            .with_context(|| format!("failed to open sqlite db {}", path.display()))?;

        sqlite_migrator()
            .run(&pool)
            .await
            .with_context(|| format!("failed to migrate sqlite db {}", path.display()))?;

        Ok(Self {
            backing: Backing::Sqlite(pool),
        })
    }

    /// Transition pool accessor for identity store code.
    pub fn identity_pool(&self) -> &SqlitePool {
        self.sqlite_pool()
    }

    /// Transition pool accessor for session store code.
    pub fn session_pool(&self) -> &SqlitePool {
        self.sqlite_pool()
    }

    /// Transition pool accessor for runtime store code.
    pub fn runtime_pool(&self) -> &SqlitePool {
        self.sqlite_pool()
    }

    fn sqlite_pool(&self) -> &SqlitePool {
        match &self.backing {
            Backing::Sqlite(pool) => pool,
            Backing::Postgres(_) => {
                panic!("transition SQLite pool accessor called on a Postgres-backed LiloDb")
            }
        }
    }
}

/// Transition: a pool connection holding an open `SQLite` `BEGIN IMMEDIATE`.
pub struct ImmediateTx {
    conn: PoolConnection<Sqlite>,
    open: bool,
}

impl ImmediateTx {
    async fn commit(mut self) -> sqlx::Result<()> {
        sqlx::query("COMMIT").execute(&mut *self).await?;
        self.open = false;
        Ok(())
    }

    async fn rollback(mut self) -> sqlx::Result<()> {
        sqlx::query("ROLLBACK").execute(&mut *self).await?;
        self.open = false;
        Ok(())
    }
}

impl Deref for ImmediateTx {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}

impl DerefMut for ImmediateTx {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.conn
    }
}

impl Drop for ImmediateTx {
    fn drop(&mut self) {
        if self.open {
            <Sqlite as Database>::TransactionManager::start_rollback(&mut self.conn);
        }
    }
}

/// Transition: begin a `SQLite` `IMMEDIATE` transaction on a borrowed connection.
pub async fn begin_immediate_tx(conn: &mut SqliteConnection, label: &str) -> Result<()> {
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *conn)
        .await
        .with_context(|| format!("failed to begin {label}"))?;
    Ok(())
}

/// Transition: commit or roll back a connection-scoped `SQLite` transaction.
pub async fn finish_immediate_tx<T>(
    conn: &mut SqliteConnection,
    result: Result<T>,
    label: &str,
) -> Result<T> {
    match result {
        Ok(value) => {
            sqlx::query("COMMIT")
                .execute(conn)
                .await
                .with_context(|| format!("failed to commit {label}"))?;
            Ok(value)
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(conn).await;
            Err(error)
        }
    }
}

/// Transition: begin a pool-scoped `SQLite` `IMMEDIATE` transaction.
pub async fn begin_immediate_pool_tx(pool: &SqlitePool) -> sqlx::Result<ImmediateTx> {
    let mut conn = pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
    Ok(ImmediateTx { conn, open: true })
}

/// Transition: commit or roll back a pool-scoped `SQLite` transaction.
pub async fn finish_immediate_pool_tx<T, E>(
    transaction: ImmediateTx,
    result: std::result::Result<T, E>,
) -> std::result::Result<T, E>
where
    E: From<sqlx::Error>,
{
    match result {
        Ok(value) => {
            transaction.commit().await.map_err(E::from)?;
            Ok(value)
        }
        Err(error) => {
            let _ = transaction.rollback().await;
            Err(error)
        }
    }
}
