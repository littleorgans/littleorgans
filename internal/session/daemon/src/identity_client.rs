use std::path::Path;

use anyhow::{Context, Result};
use lilo_im_core::{
    Action, Authorizer, Principal, ResourceSpec, RuntimeKind as IdentityRuntimeKind,
};
use lilo_im_store::SqliteAuditSink;
use lilo_im_stub::StubAuthorizer;
use lilo_session_core::{RuntimeKind, SpawnRequest};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RequestContext {
    pub principal: Principal,
    pub mcp_caller_session_id: Option<Uuid>,
}

impl RequestContext {
    pub fn new(principal: Principal) -> Self {
        Self {
            principal,
            mcp_caller_session_id: None,
        }
    }

    #[must_use]
    pub fn with_mcp_caller_session_id(mut self, id: Uuid) -> Self {
        self.mcp_caller_session_id = Some(id);
        self
    }
}

#[derive(Debug, Clone)]
pub struct IdentityClient {
    audit_sink: SqliteAuditSink,
    local_uid: u32,
}

impl IdentityClient {
    pub async fn connect_default() -> Result<Self> {
        Self::connect(lilo_im_store::default_audit_db_path(), local_uid()).await
    }

    pub async fn connect(path: impl AsRef<Path>, local_uid: u32) -> Result<Self> {
        let audit_sink = SqliteAuditSink::connect(path)
            .await
            .context("failed to connect identity audit sink")?;
        audit_sink
            .run_migrations()
            .await
            .context("failed to initialize identity audit sink")?;
        Ok(Self {
            audit_sink,
            local_uid,
        })
    }

    pub async fn authorize(
        &self,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> Result<()> {
        let authorizer = StubAuthorizer::new(&self.audit_sink, self.local_uid);
        authorizer
            .authorize(principal, action, resource)
            .await
            .map(|_| ())
            .context("authorization failed")
    }
}

pub fn spawn_resource(request: &SpawnRequest, session_id: Uuid) -> ResourceSpec {
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

pub fn session_resource(session_id: Uuid) -> ResourceSpec {
    ResourceSpec {
        session_id: Some(session_id),
        ..Default::default()
    }
}

fn identity_runtime(runtime: RuntimeKind) -> IdentityRuntimeKind {
    match runtime {
        RuntimeKind::Claude => IdentityRuntimeKind::Claude,
        RuntimeKind::Codex => IdentityRuntimeKind::Codex,
    }
}

fn local_uid() -> u32 {
    nix::unistd::getuid().as_raw()
}
