use lilo_session_store::SqliteStore;

use crate::handler::DaemonState;

impl DaemonState {
    pub(crate) fn store(&self) -> &SqliteStore {
        &self.store
    }
}
