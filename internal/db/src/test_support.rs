//! Postgres test isolation fixture.
//!
//! [`TestDb::create`] provisions a uniquely named database from an admin
//! connection, runs migrations through the Postgres open path, and hands back a
//! ready [`LiloDb`]. Cleanup is explicit and async ([`TestDb::cleanup`]); `Drop`
//! never performs async work, it only warns about a leaked database so it is
//! easy to find and drop by hand.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, ensure};
use lilo_paths::Settings;
use lilo_paths::env::resolve_test_database_url;
use sqlx::{Connection, Executor, PgConnection};

use crate::{DbConfig, LiloDb, redacted};

/// Short, greppable prefix so a leaked fixture database is easy to identify.
const TEST_DB_PREFIX: &str = "lilo_test_";
/// Actionable guidance when no admin URL is configured anywhere.
const ADMIN_URL_GUIDANCE: &str = "no admin Postgres URL for the lilo-db test fixture: set LILO_TEST_DATABASE_URL (or LILO_DATABASE_URL), or add `[database] test_url` to $LILO_HOME/settings.toml (copy settings.example.toml). Refusing to connect to a guessed host.";

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A throwaway Postgres database scoped to a single test.
pub struct TestDb {
    db: LiloDb,
    database_url: String,
    admin_url: String,
    database_name: String,
    cleaned: bool,
}

impl TestDb {
    /// Create and migrate a fresh, uniquely named Postgres test database.
    ///
    /// # Errors
    /// Returns an error if the admin connection, database creation, or migration
    /// fails.
    pub async fn create() -> Result<Self> {
        let admin_url = admin_url()?;
        let database_name = unique_db_name();

        let mut admin = PgConnection::connect(&admin_url).await.with_context(|| {
            format!(
                "failed to connect to admin database {}",
                redacted(&admin_url)
            )
        })?;
        admin
            .execute(format!("CREATE DATABASE \"{database_name}\"").as_str())
            .await
            .with_context(|| format!("failed to create test database {database_name}"))?;
        admin.close().await.ok();

        let database_url = swap_database(&admin_url, &database_name)?;
        let db = LiloDb::open_postgres(DbConfig::from_url(database_url.clone()))
            .await
            .with_context(|| format!("failed to open test database {database_name}"))?;

        Ok(Self {
            db,
            database_url,
            admin_url,
            database_name,
            cleaned: false,
        })
    }

    /// The provisioned database handle.
    pub fn db(&self) -> &LiloDb {
        &self.db
    }

    /// The connection URL for the provisioned test database.
    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    /// Close the pool and drop the test database.
    ///
    /// # Errors
    /// Returns an error if the admin reconnect or `DROP DATABASE` fails.
    pub async fn cleanup(mut self) -> Result<()> {
        self.db.close().await;

        let mut admin = PgConnection::connect(&self.admin_url)
            .await
            .with_context(|| format!("failed to reconnect admin to drop {}", self.database_name))?;
        admin
            .execute(
                format!(
                    "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
                    self.database_name
                )
                .as_str(),
            )
            .await
            .with_context(|| format!("failed to drop test database {}", self.database_name))?;
        admin.close().await.ok();

        self.cleaned = true;
        Ok(())
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if !self.cleaned {
            eprintln!(
                "warning: TestDb dropped without cleanup; leaked database {} (admin {})",
                self.database_name,
                redacted(&self.admin_url)
            );
        }
    }
}

/// Admin/maintenance URL for provisioning, resolved through env over
/// `settings.toml` (`LILO_TEST_DATABASE_URL` → `LILO_DATABASE_URL` →
/// `settings.database.test_url` → `settings.database.url`). Env wins, so the
/// suite stays hermetic. A malformed `settings.toml` is ignored here (env
/// already decides); resolution to nothing fails loud rather than guessing.
///
/// # Errors
/// Returns an actionable error when no admin URL is configured anywhere.
pub(crate) fn admin_url() -> Result<String> {
    let settings = Settings::load().unwrap_or_default();
    resolve_test_database_url(&settings).context(ADMIN_URL_GUIDANCE)
}

/// Extract the database name (last path segment) from a Postgres URL.
///
/// # Errors
/// Returns an error if the URL has no `/database` path segment.
#[cfg(test)]
pub(crate) fn database_name_of(url: &str) -> Result<String> {
    let (base, _query) = url.split_once('?').unwrap_or((url, ""));
    let scheme_end = base.find("://").context("postgres url missing scheme")?;
    let slash = base
        .rfind('/')
        .context("postgres url missing '/database'")?;
    ensure!(slash > scheme_end + 2, "postgres url missing '/database'");
    Ok(base[slash + 1..].to_string())
}

fn unique_db_name() -> String {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{TEST_DB_PREFIX}{pid}_{nanos:x}_{seq}")
}

/// Replace the database segment of a Postgres URL, preserving query parameters.
fn swap_database(url: &str, name: &str) -> Result<String> {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let scheme_end = base.find("://").context("postgres url missing scheme")?;
    let slash = base
        .rfind('/')
        .context("postgres url missing '/database'")?;
    ensure!(slash > scheme_end + 2, "postgres url missing '/database'");

    let mut out = String::with_capacity(url.len() + name.len());
    out.push_str(&base[..=slash]);
    out.push_str(name);
    if let Some(query) = query {
        out.push('?');
        out.push_str(query);
    }
    Ok(out)
}
