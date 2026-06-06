pub use lilo_identity_service::IdentityClient;

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use lilo_common::id::SessionId;
use lilo_db::ImmediateTx;
use lilo_im_core::{Action, Principal, ResourceSpec, RuntimeKind as IdentityRuntimeKind};
use lilo_session_core::{RuntimeKind, SpawnRequest};

pub type IdentityPortFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait IdentityPort: Send + Sync {
    fn authorize<'a>(
        &'a self,
        principal: &'a Principal,
        action: Action,
        resource: &'a ResourceSpec,
    ) -> IdentityPortFuture<'a, ()>;

    fn authorize_in_tx<'a>(
        &'a self,
        tx: &'a mut ImmediateTx,
        principal: &'a Principal,
        action: Action,
        resource: &'a ResourceSpec,
    ) -> IdentityPortFuture<'a, ()>;

    fn authorize_session<'a>(
        &'a self,
        principal: &'a Principal,
        action: Action,
        session_id: SessionId,
    ) -> IdentityPortFuture<'a, ()> {
        Box::pin(async move {
            let resource = session_resource(session_id);
            self.authorize(principal, action, &resource).await
        })
    }
}

impl IdentityPort for IdentityClient {
    fn authorize<'a>(
        &'a self,
        principal: &'a Principal,
        action: Action,
        resource: &'a ResourceSpec,
    ) -> IdentityPortFuture<'a, ()> {
        Box::pin(IdentityClient::authorize(self, principal, action, resource))
    }

    fn authorize_in_tx<'a>(
        &'a self,
        tx: &'a mut ImmediateTx,
        principal: &'a Principal,
        action: Action,
        resource: &'a ResourceSpec,
    ) -> IdentityPortFuture<'a, ()> {
        Box::pin(IdentityClient::authorize_in_tx(
            self, tx, principal, action, resource,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct RequestContext {
    pub principal: Principal,
    pub caller_session_id: Option<SessionId>,
}

impl RequestContext {
    pub fn new(principal: Principal) -> Self {
        Self {
            principal,
            caller_session_id: None,
        }
    }

    #[must_use]
    pub fn with_caller_session_id(mut self, id: SessionId) -> Self {
        self.caller_session_id = Some(id);
        self
    }

    #[must_use]
    pub fn with_mcp_caller_session_id(self, id: SessionId) -> Self {
        self.with_caller_session_id(id)
    }
}

pub fn spawn_resource(request: &SpawnRequest, session_id: SessionId) -> ResourceSpec {
    ResourceSpec {
        workspace: Some(request.workspace.clone()),
        role: Some(request.role.clone()),
        runtime: Some(identity_runtime(request.runtime)),
        session_id: Some(session_id),
        labels: request
            .labels
            .iter()
            .map(|label| (label.key.clone(), label.value.clone()))
            .collect(),
    }
}

pub fn session_resource(session_id: SessionId) -> ResourceSpec {
    ResourceSpec::session(session_id)
}

fn identity_runtime(runtime: RuntimeKind) -> IdentityRuntimeKind {
    match runtime {
        RuntimeKind::Claude => IdentityRuntimeKind::Claude,
        RuntimeKind::Codex => IdentityRuntimeKind::Codex,
    }
}
