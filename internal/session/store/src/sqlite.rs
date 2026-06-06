mod events;
mod labels;
mod mail;
#[cfg(test)]
mod mail_tests;
mod namespaces;
mod sessions;
mod spawn_intents;
#[cfg(test)]
mod test_support;
mod time;

use lilo_db::{ImmediateTx, LiloDb, begin_immediate_pool_tx};
use sqlx::SqlitePool;

pub use mail::{MailRowError, MailWriteOutcome};
pub use namespaces::{NamespaceRecord, NamespaceRowError, SessionNamespace};
pub use sessions::SessionRowError;
pub use spawn_intents::{
    PendingSpawnIntent, SessionDraft, SessionSpawnIntent, SpawnIntentError, SpawnIntentStatus,
};

#[derive(Clone)]
pub struct SessionStore {
    pool: SqlitePool,
}

impl SessionStore {
    #[must_use]
    pub fn from_db(db: &LiloDb) -> Self {
        Self {
            pool: db.session_pool().clone(),
        }
    }

    /// Transition: crate-private `SQLite` pool accessor (Phase 2 removes `SQLite`).
    #[must_use]
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Begin a pool-scoped immediate transaction for the shared spawn path.
    ///
    /// Returns the backend-neutral [`ImmediateTx`] handle threaded across
    /// crates; the caller commits it with `finish_immediate_pool_tx`. This is
    /// the single transaction mechanism for the spawn path: do not also call
    /// the connection-scoped `begin_immediate_tx`/`finish_immediate_tx` on this
    /// handle.
    pub async fn begin_immediate_tx(&self) -> sqlx::Result<ImmediateTx> {
        begin_immediate_pool_tx(&self.pool).await
    }

    #[cfg(test)]
    pub async fn open_temp() -> (tempfile::TempDir, Self) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db = LiloDb::open_path(dir.path().join("lilo.db"))
            .await
            .expect("open lilo db");
        (dir, Self::from_db(&db))
    }
}
