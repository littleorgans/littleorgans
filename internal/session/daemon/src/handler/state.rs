use std::sync::Arc;

use lilo_runtime_daemon::RuntimeService;
use lilo_session_core::RpcResponse;
use lilo_session_driver::RuntimePort;
use lilo_session_store::SqliteStore;

use crate::identity_client::IdentityClient;

pub struct DaemonState {
    pub store: SqliteStore,
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
        runtime: Arc<dyn RuntimePort>,
        identity: Arc<IdentityClient>,
        runtime_service: Arc<RuntimeService>,
    ) -> Self {
        Self {
            store,
            runtime,
            runtime_service,
            identity,
        }
    }
}
