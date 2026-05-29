use std::sync::Arc;

use lilo_runtime_daemon::RuntimeService;
use lilo_session_core::RpcResponse;
use lilo_session_driver::RuntimePort;
use lilo_session_store::SqliteStore;

use crate::identity_client::IdentityClient;

pub struct DaemonState {
    pub store: SqliteStore,
    pub(crate) daemon_version: String,
    pub(crate) runtime: Arc<dyn RuntimePort>,
    pub(crate) runtime_service: Arc<RuntimeService>,
    pub(crate) identity: Arc<IdentityClient>,
}

pub struct HandlerResult {
    pub response: RpcResponse,
    pub shutdown: bool,
}

impl DaemonState {
    pub fn new(
        store: SqliteStore,
        daemon_version: impl Into<String>,
        runtime: Arc<dyn RuntimePort>,
        identity: Arc<IdentityClient>,
        runtime_service: Arc<RuntimeService>,
    ) -> Self {
        Self {
            store,
            daemon_version: daemon_version.into(),
            runtime,
            runtime_service,
            identity,
        }
    }
}
