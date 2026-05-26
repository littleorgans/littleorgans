use std::path::PathBuf;

use async_trait::async_trait;
use lilo_im_core::{Action, AuditRow, Authorizer, AuthzResult, Principal, ResourceSpec};
use lilo_im_store::{AuditFilters, SqliteAuditSink, StoreError};
use lilo_im_stub::StubAuthorizer;

pub type Result<T> = std::result::Result<T, IdentityServiceError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityConfig {
    pub audit_db_path: PathBuf,
    pub local_uid: u32,
}

impl IdentityConfig {
    #[must_use]
    pub fn new(audit_db_path: impl Into<PathBuf>, local_uid: u32) -> Self {
        Self {
            audit_db_path: audit_db_path.into(),
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
    audit_sink: SqliteAuditSink,
    local_uid: u32,
}

impl IdentityService {
    pub async fn build(config: IdentityConfig) -> Result<Self> {
        let audit_sink = SqliteAuditSink::connect(config.audit_db_path).await?;
        audit_sink.run_migrations().await?;

        Ok(Self {
            audit_sink,
            local_uid: config.local_uid,
        })
    }

    #[must_use]
    pub fn local_uid(&self) -> u32 {
        self.local_uid
    }

    #[must_use]
    pub fn audit_sink(&self) -> &SqliteAuditSink {
        &self.audit_sink
    }

    pub async fn authorize(
        &self,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> AuthzResult {
        self.authorize_with_stub(principal, action, resource).await
    }

    pub async fn query_audit(&self, filters: AuditFilters) -> Result<Vec<AuditRow>> {
        Ok(self.audit_sink.query_audit(filters).await?)
    }

    fn authorizer(&self) -> StubAuthorizer<'_, SqliteAuditSink> {
        StubAuthorizer::new(&self.audit_sink, self.local_uid)
    }

    async fn authorize_with_stub(
        &self,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> AuthzResult {
        self.authorizer()
            .authorize(principal, action, resource)
            .await
    }
}

#[async_trait]
impl Authorizer for IdentityService {
    async fn authorize(
        &self,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> AuthzResult {
        self.authorize_with_stub(principal, action, resource).await
    }
}
