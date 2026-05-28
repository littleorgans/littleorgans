pub use lilo_identity_service::IdentityClient;

use lilo_im_core::{Principal, ResourceSpec, RuntimeKind as IdentityRuntimeKind};
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
