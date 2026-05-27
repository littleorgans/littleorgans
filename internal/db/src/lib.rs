#![deny(unsafe_code)]

use std::time::Duration;

use anyhow::{Context, Result};
use lilo_paths::LiloPaths;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: u32 = 5;
const WAL_AUTOCHECKPOINT_PAGES: &str = "1000";

#[derive(Clone)]
pub struct LiloDb {
    pool: SqlitePool,
}

impl LiloDb {
    pub async fn open(paths: &LiloPaths) -> Result<Self> {
        let path = paths.db_path();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!("failed to create lilo db directory {}", parent.display())
            })?;
        }

        let options = SqliteConnectOptions::new()
            .filename(&path)
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

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .with_context(|| format!("failed to migrate sqlite db {}", path.display()))?;

        Ok(Self { pool })
    }

    pub fn identity_pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn session_pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn runtime_pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use lilo_paths::{LiloHome, LiloPaths};
    use tempfile::TempDir;

    #[tokio::test]
    async fn open_creates_unified_schema_tables() -> Result<()> {
        let (_dir, db) = open_temp_db().await?;
        let tables = schema_tables(&db).await?;

        assert_eq!(
            tables,
            BTreeSet::from([
                "identity_audit".to_string(),
                "runtime_lifecycle".to_string(),
                "runtime_metadata".to_string(),
                "session_event_cursor".to_string(),
                "session_labels".to_string(),
                "session_mail".to_string(),
                "session_namespaces".to_string(),
                "session_sessions".to_string(),
                "session_spawn_intents".to_string(),
            ])
        );
        Ok(())
    }

    #[tokio::test]
    async fn open_applies_sqlite_pragmas_with_wire_values() -> Result<()> {
        let (_dir, db) = open_temp_db().await?;
        let pool = db.session_pool();

        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(pool)
            .await?;
        let busy_timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(pool)
            .await?;
        let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
            .fetch_one(pool)
            .await?;

        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
        assert_eq!(busy_timeout, 5_000);
        assert_eq!(synchronous, 1);
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_accessor_writes_complete_without_busy_errors() -> Result<()> {
        let (_dir, db) = open_temp_db().await?;
        let identity_pool = db.identity_pool().clone();
        let runtime_pool = db.runtime_pool().clone();

        let identity_writes = tokio::spawn(async move {
            for index in 0..25 {
                sqlx::query(
                    r"
                    INSERT INTO identity_audit (
                        id, timestamp, principal, action, resource, decision
                    ) VALUES (?, ?, ?, ?, ?, ?)
                    ",
                )
                .bind(format!("audit-{index}"))
                .bind("2026-05-27T00:00:00Z")
                .bind("operator:stuart")
                .bind("allow")
                .bind(format!("runtime:{index}"))
                .bind("allow")
                .execute(&identity_pool)
                .await?;
            }
            Result::<()>::Ok(())
        });
        let runtime_writes = tokio::spawn(async move {
            for index in 0..25 {
                sqlx::query(
                    r"
                    INSERT INTO runtime_metadata (key, value, updated_at)
                    VALUES (?, ?, ?)
                    ",
                )
                .bind(format!("key-{index}"))
                .bind(format!("value-{index}"))
                .bind("2026-05-27T00:00:00Z")
                .execute(&runtime_pool)
                .await?;
            }
            Result::<()>::Ok(())
        });

        identity_writes.await??;
        runtime_writes.await??;
        Ok(())
    }

    async fn open_temp_db() -> Result<(TempDir, LiloDb)> {
        let dir = tempfile::tempdir()?;
        let home = LiloHome::from_path(dir.path().join("lilo"))?;
        let paths = LiloPaths::new(home);
        let db = LiloDb::open(&paths).await?;
        Ok((dir, db))
    }

    async fn schema_tables(db: &LiloDb) -> Result<BTreeSet<String>> {
        let tables = sqlx::query_scalar(
            r"
            SELECT name
            FROM sqlite_master
            WHERE type = 'table'
              AND name NOT LIKE '\_%' ESCAPE '\'
            ORDER BY name
            ",
        )
        .fetch_all(db.session_pool())
        .await?;
        Ok(tables.into_iter().collect())
    }
}
