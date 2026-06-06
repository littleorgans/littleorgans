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

use lilo_db::LiloDb;
use sqlx::PgPool;

pub use mail::{MailRowError, MailWriteOutcome};
pub use namespaces::{NamespaceRecord, NamespaceRowError, SessionNamespace};
pub use sessions::SessionRowError;
pub use spawn_intents::{
    PendingSpawnIntent, SessionDraft, SessionSpawnIntent, SpawnIntentError, SpawnIntentStatus,
};

#[derive(Clone)]
pub struct SessionStore {
    pool: PgPool,
}

impl SessionStore {
    #[must_use]
    pub fn from_db(db: &LiloDb) -> Self {
        Self {
            pool: db.pool().clone(),
        }
    }

    /// Crate-private Postgres pool accessor for tests that issue raw SQL.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Begin a pool-scoped transaction for the shared spawn path.
    ///
    /// Returns the backend-neutral [`lilo_db::LiloTransaction`] handle threaded
    /// across crates; the caller commits it with [`lilo_db::commit_or_rollback`].
    pub async fn begin_tx(&self) -> sqlx::Result<lilo_db::LiloTransaction<'_>> {
        self.pool.begin().await
    }
}
