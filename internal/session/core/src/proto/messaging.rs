use serde::{Deserialize, Serialize};

use super::TargetError;
use crate::{MailCountView, MailIntent, MailNotifyMode, MailSendResult, MessageView, Selector};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailSendRequest {
    pub to: Selector,
    pub content: String,
    pub notify: Option<MailNotifyMode>,
    pub context_id: String,
    pub intent: MailIntent,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailSendResponse {
    pub results: Vec<MailSendResult>,
    #[serde(default)]
    pub errors: Vec<TargetError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailReadRequest {
    pub selector: Selector,
    pub peek: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailReadResponse {
    pub messages: Vec<MessageView>,
    #[serde(default)]
    pub errors: Vec<TargetError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailCheckRequest {
    pub selector: Selector,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailCheckResponse {
    pub unread: usize,
    pub counts: Vec<MailCountView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStopCheckRequest {
    pub selector: Selector,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStopCheckResponse {
    pub unread: usize,
    pub counts: Vec<MailCountView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NudgeRequest {
    pub to: Selector,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NudgeResponse {
    pub nudges: Vec<NudgeDelivery>,
    #[serde(default)]
    pub errors: Vec<TargetError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NudgeDelivery {
    pub to: String,
    pub delivered: bool,
    pub message: String,
}
