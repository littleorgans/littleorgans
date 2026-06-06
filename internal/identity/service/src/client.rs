use anyhow::{Context, Result};
use lilo_db::{LiloDb, LiloTransaction};
use lilo_im_core::{
    Action, AuditDecision, AuditRow, Authorizer, AuthzError, Principal, ResourceSpec,
};
use lilo_im_store::AuditStore;
use lilo_im_store::postgres::record_audit_in_tx;
use lilo_im_stub::StubAuthorizer;

#[derive(Debug, Clone)]
pub struct IdentityClient {
    audit_sink: AuditStore,
    local_uid: u32,
}

impl IdentityClient {
    #[must_use]
    pub fn new(audit_sink: AuditStore, local_uid: u32) -> Self {
        Self {
            audit_sink,
            local_uid,
        }
    }

    #[must_use]
    pub fn from_db(db: &LiloDb, local_uid: u32) -> Self {
        Self::new(AuditStore::with_pool(db.pool().clone()), local_uid)
    }

    #[must_use]
    pub fn local_uid(&self) -> u32 {
        self.local_uid
    }

    #[must_use]
    pub fn audit_sink(&self) -> &AuditStore {
        &self.audit_sink
    }

    pub async fn authorize(
        &self,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> Result<()> {
        self.authorize_with_stub(principal, action, resource)
            .await
            .map(|_| ())
            .context("authorization failed")
    }

    pub async fn authorize_in_tx(
        &self,
        tx: &mut LiloTransaction<'_>,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> Result<()> {
        let decision = AuditDecision::evaluate_local(principal, self.local_uid);
        let row = AuditRow::new(
            principal.clone(),
            action,
            resource.clone(),
            decision.clone(),
        );
        record_audit_in_tx(tx, &row)
            .await
            .context("authorization failed")?;
        if decision == AuditDecision::Allow {
            Ok(())
        } else {
            Err(AuthzError::UnknownPrincipal).context("authorization failed")
        }
    }

    pub(crate) fn authorizer(&self) -> StubAuthorizer<'_, AuditStore> {
        StubAuthorizer::new(&self.audit_sink, self.local_uid)
    }

    pub(crate) async fn authorize_with_stub(
        &self,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> lilo_im_core::AuthzResult {
        self.authorizer()
            .authorize(principal, action, resource)
            .await
    }
}
