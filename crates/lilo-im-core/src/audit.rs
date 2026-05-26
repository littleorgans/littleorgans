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
