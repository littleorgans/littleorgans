use lilo_session_store::SessionStore;

use crate::handler::DaemonState;

impl DaemonState {
    pub(crate) fn store(&self) -> &SessionStore {
        &self.store
    }
}
