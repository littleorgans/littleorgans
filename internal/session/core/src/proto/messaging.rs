use serde::{Deserialize, Serialize};

use super::TargetError;
use crate::{MailCountView, MailIntent, MailNotifyMode, MailSendResult, MessageView, Selector};
use chrono::{DateTime, Utc};
use lilo_common::id::MessageId;
use lilo_rm_core::NudgeMode;

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
pub struct MailReadRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailReadResponse {
    pub messages: Vec<MessageView>,
    #[serde(default)]
    pub errors: Vec<TargetError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MailLogFilter {
    #[serde(default)]
    pub context_id: Option<String>,
    #[serde(default)]
    pub selector: Option<Selector>,
    #[serde(default)]
    pub recipient: Option<Selector>,
    #[serde(default)]
    pub include_system: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailLogCursor {
    pub sent_at: DateTime<Utc>,
    pub message_id: MessageId,
}

impl MailLogCursor {
    pub fn from_message(message: &MessageView) -> Self {
        Self {
            sent_at: message.sent_at,
            message_id: message.id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailPeekRequest {
    pub filter: MailLogFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailPeekResponse {
    pub messages: Vec<MessageView>,
    pub cursor: Option<MailLogCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailTailRequest {
    pub filter: MailLogFilter,
    #[serde(default)]
    pub after: Option<MailLogCursor>,
    #[serde(default)]
    pub follow: bool,
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailTailResponse {
    pub messages: Vec<MessageView>,
    pub cursor: Option<MailLogCursor>,
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
    pub mode: NudgeMode,
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
