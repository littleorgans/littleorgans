use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AuditError;
use crate::types::{Action, Principal, ResourceSpec};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuditDecision {
    Allow,
    Deny { reason: String },
    Error { message: String },
}

impl AuditDecision {
    /// Evaluate the v1 local-uid authorization rule.
    ///
    /// A `Principal::Local` whose uid equals `local_uid` is allowed. Every
    /// other principal is denied with a stable reason string. This is the one
    /// home for the v1 rule and its denial reasons, so the stub authorizer and
    /// the in-transaction client path both call it and cannot silently
    /// diverge. Only `Allow` and `Deny` are produced; the rule never yields
    /// `Error`.
    #[must_use]
    pub fn evaluate_local(principal: &Principal, local_uid: u32) -> Self {
        match principal {
            Principal::Local(uid) if *uid == local_uid => Self::Allow,
            Principal::Local(_) => Self::Deny {
                reason: "non-local uid".to_owned(),
            },
            Principal::Unknown { .. } => Self::Deny {
                reason: "unknown principal".to_owned(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRow {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub principal: Principal,
    pub action: Action,
    pub resource: ResourceSpec,
    pub decision: AuditDecision,
    pub session_ref: Option<Uuid>,
    pub notes: Option<String>,
    pub policy_id: Option<String>,
    pub evaluation_trace: Option<String>,
    pub denial_reason: Option<String>,
}

impl AuditRow {
    #[must_use]
    pub fn new(
        principal: Principal,
        action: Action,
        resource: ResourceSpec,
        decision: AuditDecision,
    ) -> Self {
        let denial_reason = match &decision {
            AuditDecision::Deny { reason } => Some(reason.clone()),
            _ => None,
        };

        Self {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            principal,
            action,
            session_ref: resource.session_id,
            resource,
            decision,
            notes: None,
            policy_id: None,
            evaluation_trace: None,
            denial_reason,
        }
    }
}

pub trait AuditSink: Send + Sync {
    fn record(
        &self,
        row: AuditRow,
    ) -> impl std::future::Future<Output = Result<(), AuditError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL_UID: u32 = 501;

    #[test]
    fn evaluate_local_allows_matching_local_uid() {
        let decision = AuditDecision::evaluate_local(&Principal::local(LOCAL_UID), LOCAL_UID);
        assert_eq!(decision, AuditDecision::Allow);
    }

    #[test]
    fn evaluate_local_denies_non_matching_local_uid() {
        let decision = AuditDecision::evaluate_local(&Principal::local(LOCAL_UID + 1), LOCAL_UID);
        assert_eq!(
            decision,
            AuditDecision::Deny {
                reason: "non-local uid".to_owned(),
            }
        );
    }

    #[test]
    fn evaluate_local_denies_unknown_principal() {
        let principal = Principal::Unknown {
            kind: "test".to_owned(),
            raw: serde_json::Value::Null,
        };
        let decision = AuditDecision::evaluate_local(&principal, LOCAL_UID);
        assert_eq!(
            decision,
            AuditDecision::Deny {
                reason: "unknown principal".to_owned(),
            }
        );
    }
}
