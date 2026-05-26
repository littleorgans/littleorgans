//! Identity Matters v1 stub: `StubAuthorizer` audits every decision without
//! enforcement. Use this crate to lock the `lilo-im-core` boundary at call
//! sites today; v2+ swaps it for `lilo-im-daemon` behind the same `Authorizer`.

use async_trait::async_trait;
use lilo_im_core::{
    Action, AuditDecision, AuditRow, AuditSink, Authorized, Authorizer, AuthzError, AuthzResult,
    Principal, ResourceSpec,
};

pub struct StubAuthorizer<'a, S: AuditSink + ?Sized> {
    pub audit_sink: &'a S,
    pub local_uid: u32,
}

impl<'a, S: AuditSink + ?Sized> StubAuthorizer<'a, S> {
    #[must_use]
    pub fn new(audit_sink: &'a S, local_uid: u32) -> Self {
        Self {
            audit_sink,
            local_uid,
        }
    }

    async fn record(
        &self,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
        decision: AuditDecision,
    ) -> Result<(), AuthzError> {
        let row = AuditRow::new(principal.clone(), action, resource.clone(), decision);
        self.audit_sink.record(row).await.map_err(AuthzError::audit)
    }
}

#[async_trait]
impl<S> Authorizer for StubAuthorizer<'_, S>
where
    S: AuditSink + ?Sized,
{
    async fn authorize(
        &self,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> AuthzResult {
        if *principal == Principal::Local(self.local_uid) {
            self.record(principal, action, resource, AuditDecision::Allow)
                .await?;
            return Ok(Authorized {
                principal: principal.clone(),
                role: "admin".to_owned(),
                capabilities: Vec::new(),
            });
        }

        let reason = match principal {
            Principal::Local(_) => "non-local uid",
            Principal::Unknown { .. } => "unknown principal",
        };
        self.record(
            principal,
            action,
            resource,
            AuditDecision::Deny {
                reason: reason.to_owned(),
            },
        )
        .await?;

        Err(AuthzError::UnknownPrincipal)
    }
}
