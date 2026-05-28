mod events;
mod labels;
mod mail;
mod namespaces;
mod sessions;
mod spawn_intents;
#[cfg(test)]
mod test_support;
mod time;

use lilo_db::LiloDb;
use sqlx::SqlitePool;

pub use mail::MailRowError;
pub use namespaces::{NamespaceRecord, NamespaceRowError, SessionNamespace};
pub use sessions::SessionRowError;
pub use spawn_intents::{
    PendingSpawnIntent, SessionDraft, SessionSpawnIntent, SpawnIntentError, SpawnIntentStatus,
};

#[derive(Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    #[must_use]
    pub fn open(db: &LiloDb) -> Self {
        Self {
            pool: db.session_pool().clone(),
        }
    }

    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    #[cfg(test)]
    pub async fn open_temp() -> (tempfile::TempDir, Self) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db = LiloDb::open_path(dir.path().join("lilo.db"))
            .await
            .expect("open lilo db");
        (dir, Self::open(&db))
    }
}
