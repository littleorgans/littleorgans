use std::path::PathBuf;
use std::sync::Arc;

use lilo_runtime_daemon::RuntimeService;
use lilo_session_core::RpcResponse;
use lilo_session_driver::SessionDriver;
use lilo_session_store::SqliteStore;

use crate::identity_client::IdentityClient;

pub struct DaemonState {
    pub store: SqliteStore,
    pub driver: Arc<dyn SessionDriver>,
    pub(crate) runtime: Arc<RuntimeService>,
    pub(crate) identity: Arc<IdentityClient>,
    pub(crate) rtmd_socket_path: Option<PathBuf>,
}

pub struct HandlerResult {
    pub response: RpcResponse,
    pub shutdown: bool,
}

impl DaemonState {
    pub fn new(
        store: SqliteStore,
        driver: Arc<dyn SessionDriver>,
        identity: Arc<IdentityClient>,
        runtime: Arc<RuntimeService>,
    ) -> Self {
        Self {
            store,
            driver,
            runtime,
            identity,
            rtmd_socket_path: None,
        }
    }

    #[must_use]
    pub fn with_rtmd_socket_path(mut self, socket_path: PathBuf) -> Self {
        self.rtmd_socket_path = Some(socket_path);
        self
    }
}
