use anyhow::{Context, Result};
use lilo_db::LiloDb;
use lilo_im_core::{
    Action, AuditDecision, AuditRow, Authorizer, AuthzError, Principal, ResourceSpec,
};
use lilo_im_store::SqliteAuditSink;
use lilo_im_store::sqlite::record_audit_in_tx;
use lilo_im_stub::StubAuthorizer;
use sqlx::SqliteConnection;

#[derive(Debug, Clone)]
pub struct IdentityClient {
    audit_sink: SqliteAuditSink,
    local_uid: u32,
}

impl IdentityClient {
    #[must_use]
    pub fn new(audit_sink: SqliteAuditSink, local_uid: u32) -> Self {
        Self {
            audit_sink,
            local_uid,
        }
    }

    #[must_use]
    pub fn from_db(db: &LiloDb, local_uid: u32) -> Self {
        Self::new(
            SqliteAuditSink::with_pool(db.identity_pool().clone()),
            local_uid,
        )
    }

    pub async fn connect(path: impl AsRef<std::path::Path>, local_uid: u32) -> Result<Self> {
        let db = LiloDb::open_path(path)
            .await
            .context("failed to open identity audit database")?;
        Ok(Self::from_db(&db, local_uid))
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
    ) -> Result<()> {
        self.authorize_with_stub(principal, action, resource)
            .await
            .map(|_| ())
            .context("authorization failed")
    }

    pub async fn authorize_in_tx(
        &self,
        conn: &mut SqliteConnection,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> Result<()> {
        let decision = self.audit_decision(principal);
        let row = AuditRow::new(principal.clone(), action, resource.clone(), decision);
        record_audit_in_tx(conn, &row)
            .await
            .context("authorization failed")?;
        if *principal == Principal::Local(self.local_uid) {
            Ok(())
        } else {
            Err(AuthzError::UnknownPrincipal).context("authorization failed")
        }
    }

    pub(crate) fn authorizer(&self) -> StubAuthorizer<'_, SqliteAuditSink> {
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

    fn audit_decision(&self, principal: &Principal) -> AuditDecision {
        if *principal == Principal::Local(self.local_uid) {
            return AuditDecision::Allow;
        }
        AuditDecision::Deny {
            reason: denial_reason(principal).to_owned(),
        }
    }
}

fn denial_reason(principal: &Principal) -> &'static str {
    match principal {
        Principal::Local(_) => "non-local uid",
        Principal::Unknown { .. } => "unknown principal",
    }
}
