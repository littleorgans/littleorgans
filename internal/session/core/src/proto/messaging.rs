use serde::{Deserialize, Serialize};

use super::TargetError;
use crate::{Mail, Selector};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailSendRequest {
    pub from: Option<String>,
    pub to: Selector,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailSendResponse {
    pub mail: Vec<Mail>,
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
    pub mail: Vec<Mail>,
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
    pub counts: Vec<MailUnreadCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStopCheckRequest {
    pub selector: Selector,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStopCheckResponse {
    pub unread: usize,
    pub counts: Vec<MailUnreadCount>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailUnreadCount {
    pub session_id: String,
    pub unread: usize,
}
