use anyhow::{Context, Result};
use lilo_im_core::{Action, ResourceSpec};
use lilo_im_store::AuditFilters;
use lilo_session_core::{
    IdentityAuditRequest, IdentityAuditResponse, IdentityWhoamiRequest, IdentityWhoamiResponse,
    RpcResponse,
};

use crate::identity_client::RequestContext;

use super::DaemonState;

impl DaemonState {
    pub(super) async fn identity_whoami(
        &self,
        context: &RequestContext,
        _request: IdentityWhoamiRequest,
    ) -> Result<RpcResponse> {
        self.identity
            .authorize(&context.principal, Action::Read, &ResourceSpec::default())
            .await?;
        Ok(RpcResponse::IdentityWhoami {
            response: IdentityWhoamiResponse {
                principal: context.principal.clone(),
            },
        })
    }

    pub(super) async fn identity_audit(
        &self,
        context: &RequestContext,
        request: IdentityAuditRequest,
    ) -> Result<RpcResponse> {
        self.identity
            .authorize(&context.principal, Action::Read, &ResourceSpec::default())
            .await?;
        let rows = self
            .identity
            .audit_sink()
            .query_audit(audit_filters(request))
            .await
            .context("failed to query identity audit")?;

        Ok(RpcResponse::IdentityAudit {
            response: IdentityAuditResponse { rows },
        })
    }
}

fn audit_filters(request: IdentityAuditRequest) -> AuditFilters {
    AuditFilters {
        principal: request.principal,
        action: request.action,
        since: request.since,
        limit: request.limit,
    }
}
