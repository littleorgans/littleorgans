use anyhow::{Context, Result};
use sqlx::SqlitePool;

pub async fn migrate(pool: &SqlitePool) -> Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("failed to run rtm-store migrations")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownMigration {
    pub version: i64,
    pub description: String,
}

pub fn known_migrations() -> Vec<KnownMigration> {
    sqlx::migrate!("./migrations")
        .iter()
        .map(|migration| KnownMigration {
            version: migration.version,
            description: migration.description.to_string(),
        })
        .collect()
}
