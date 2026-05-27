use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result};
use lilo_db::LiloDb;
use lilo_im_core::Principal;
use lilo_im_store::SqliteAuditSink;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_session_core::{RpcResponse, SessionRpc};
use lilo_session_driver::RtmdDriver;
use lilo_session_store::SqliteStore;

use crate::handler::{DaemonState, HandlerResult};
use crate::identity_client::{IdentityClient, RequestContext};
use crate::{events::RuntimeEventTask, lifecycle::LifecycleTask};

pub struct SessionServiceContext {
    paths: LiloPaths,
    db: LiloDb,
}

impl SessionServiceContext {
    pub fn new(paths: LiloPaths, db: LiloDb) -> Self {
        Self { paths, db }
    }

    pub async fn from_env() -> Result<Self> {
        let home = LiloHome::from_env().context("failed to resolve lilo home")?;
        let paths = LiloPaths::new(home);
        let db = LiloDb::open(&paths).await?;
        Ok(Self::new(paths, db))
    }

    pub fn paths(&self) -> &LiloPaths {
        &self.paths
    }

    pub fn into_parts(self) -> (LiloPaths, LiloDb) {
        (self.paths, self.db)
    }
}

pub struct SessionService {
    paths: LiloPaths,
    state: Arc<DaemonState>,
    lifecycle: LifecycleTask,
    events: RuntimeEventTask,
}

impl SessionService {
    pub fn build(ctx: SessionServiceContext) -> Result<Self> {
        let (paths, db) = ctx.into_parts();
        fs::create_dir_all(paths.run_root()).context("failed to create run directory")?;
        let store = SqliteStore::open(&db);
        let driver = RtmdDriver::new(paths.socket_path());
        let identity = IdentityClient::new(
            SqliteAuditSink::with_pool(db.identity_pool().clone()),
            nix::unistd::getuid().as_raw(),
        );
        let state = Arc::new(
            DaemonState::new(store, Arc::new(driver), Arc::new(identity))
                .with_rtmd_socket_path(paths.socket_path()),
        );
        let lifecycle = LifecycleTask::spawn(Arc::clone(&state));
        let events = RuntimeEventTask::spawn(Arc::clone(&state), paths.socket_path());
        Ok(Self {
            paths,
            state,
            lifecycle,
            events,
        })
    }

    pub fn paths(&self) -> &LiloPaths {
        &self.paths
    }

    pub async fn handle_rpc(&self, principal: Principal, request: SessionRpc) -> HandlerResult {
        self.state
            .handle(RequestContext::new(principal), request)
            .await
    }

    pub fn shutdown_response(message: impl Into<String>) -> HandlerResult {
        HandlerResult {
            response: RpcResponse::Error {
                message: message.into(),
            },
            shutdown: false,
        }
    }
}

impl Drop for SessionService {
    fn drop(&mut self) {
        let _ = &self.lifecycle;
        let _ = &self.events;
        self.state.driver.terminate_all();
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionService, SessionServiceContext};
    use lilo_db::LiloDb;
    use lilo_paths::{LiloHome, LiloPaths};

    #[tokio::test]
    async fn build_preserves_session_paths_for_later_composition() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = LiloPaths::new(LiloHome::from_path(dir.path().join("lilo")).expect("home"));
        let db = LiloDb::open(&paths).await.expect("db");

        let service = SessionService::build(SessionServiceContext::new(paths.clone(), db))
            .expect("service builds");

        assert_eq!(service.paths().socket_path(), paths.socket_path());
    }
}
