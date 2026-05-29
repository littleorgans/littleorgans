use chrono::{DateTime, Utc};
use lilo_im_core::{Action, AuditRow, Principal};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityWhoamiRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityWhoamiResponse {
    pub principal: Principal,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityAuditRequest {
    pub principal: Option<Principal>,
    pub action: Option<Action>,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityAuditResponse {
    pub rows: Vec<AuditRow>,
}
