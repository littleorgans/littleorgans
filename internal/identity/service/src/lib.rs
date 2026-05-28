mod client;

pub use client::IdentityClient;

use lilo_im_core::{Action, AuditRow, Authorizer, AuthzResult, Principal, ResourceSpec};
use lilo_im_store::{AuditFilters, SqliteAuditSink, StoreError};

pub type Result<T> = std::result::Result<T, IdentityServiceError>;

#[derive(Debug, Clone)]
pub struct IdentityConfig {
    pub audit_sink: SqliteAuditSink,
    pub local_uid: u32,
}

impl IdentityConfig {
    #[must_use]
    pub fn new(audit_sink: SqliteAuditSink, local_uid: u32) -> Self {
        Self {
            audit_sink,
            local_uid,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityServiceError {
    #[error("audit store failed: {0}")]
    Store(#[from] StoreError),
}

#[derive(Debug)]
pub struct IdentityService {
    client: IdentityClient,
}

impl IdentityService {
    #[must_use]
    pub fn build(config: IdentityConfig) -> Self {
        Self {
            client: IdentityClient::new(config.audit_sink, config.local_uid),
        }
    }

    #[must_use]
    pub fn local_uid(&self) -> u32 {
        self.client.local_uid()
    }

    #[must_use]
    pub fn audit_sink(&self) -> &SqliteAuditSink {
        self.client.audit_sink()
    }

    pub async fn authorize(
        &self,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> AuthzResult {
        self.client
            .authorize_with_stub(principal, action, resource)
            .await
    }

    pub async fn query_audit(&self, filters: AuditFilters) -> Result<Vec<AuditRow>> {
        Ok(self.client.audit_sink().query_audit(filters).await?)
    }
}

impl Authorizer for IdentityService {
    async fn authorize(
        &self,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> AuthzResult {
        self.client
            .authorize_with_stub(principal, action, resource)
            .await
    }
}
