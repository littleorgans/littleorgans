//! Identity Matters core: `Authorizer` trait, `Principal` types, peer credential
//! extraction. Authorization is NOT enforced in v1; the v2+ roadmap replaces
//! `lilo-im-stub` with an enforcing `lilo-im-daemon` behind the same contract.

pub mod audit;
pub mod error;
pub mod peer_creds;
pub mod types;

pub use audit::{AuditDecision, AuditRow, AuditSink};
pub use error::{AuditError, AuthzError};
pub use types::{Action, Authorized, Capability, Principal, ResourceSpec, RuntimeKind};

pub type AuthzResult = Result<Authorized, AuthzError>;

pub trait Authorizer: Send + Sync {
    fn authorize(
        &self,
        principal: &Principal,
        action: Action,
        resource: &ResourceSpec,
    ) -> impl std::future::Future<Output = AuthzResult> + Send;
}
