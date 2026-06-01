use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::label::Label;
use crate::namespace::Namespace;
use crate::session::Session;
use crate::{SmError, SmResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailStatus {
    Unread,
    Read,
    /// Recipient session terminated before the mail was read; it can no longer
    /// be delivered.
    Undeliverable,
}

impl fmt::Display for MailStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unread => f.write_str("unread"),
            Self::Read => f.write_str("read"),
            Self::Undeliverable => f.write_str("undeliverable"),
        }
    }
}

impl FromStr for MailStatus {
    type Err = SmError;

    fn from_str(value: &str) -> SmResult<Self> {
        match value {
            "unread" => Ok(Self::Unread),
            "read" => Ok(Self::Read),
            "undeliverable" => Ok(Self::Undeliverable),
            other => Err(SmError::Message(format!(
                "unsupported mail status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailIntent {
    Request,
    Result,
    Inform,
    Receipt,
}

impl MailIntent {
    pub fn from_client_send_str(value: &str) -> SmResult<Self> {
        Self::from_str(value)?.ensure_client_send_allowed()
    }

    pub fn ensure_client_send_allowed(self) -> SmResult<Self> {
        match self {
            Self::Receipt => Err(SmError::Message(
                "mail intent receipt is reserved for daemon system messages".to_string(),
            )),
            intent => Ok(intent),
        }
    }
}

impl fmt::Display for MailIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request => f.write_str("request"),
            Self::Result => f.write_str("result"),
            Self::Inform => f.write_str("inform"),
            Self::Receipt => f.write_str("receipt"),
        }
    }
}

impl FromStr for MailIntent {
    type Err = SmError;

    fn from_str(value: &str) -> SmResult<Self> {
        match value {
            "request" => Ok(Self::Request),
            "result" => Ok(Self::Result),
            "inform" => Ok(Self::Inform),
            "receipt" => Ok(Self::Receipt),
            other => Err(SmError::Message(format!(
                "unsupported mail intent: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailNotifyMode {
    Wait,
    Steer,
}

impl MailNotifyMode {
    pub const WAIT_VALUE: &'static str = "wait";
    pub const STEER_VALUE: &'static str = "steer";
    pub const CLIENT_VALUES: &'static [&'static str] = &[Self::WAIT_VALUE, Self::STEER_VALUE];
}

impl FromStr for MailNotifyMode {
    type Err = SmError;

    fn from_str(value: &str) -> SmResult<Self> {
        match value {
            Self::WAIT_VALUE => Ok(Self::Wait),
            Self::STEER_VALUE => Ok(Self::Steer),
            other => Err(SmError::Message(format!(
                "unsupported mail notify mode: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SenderRef {
    Session { session_id: Uuid },
    Operator { principal: Value },
    System,
}

impl SenderRef {
    pub fn session(session_id: Uuid) -> Self {
        Self::Session { session_id }
    }

    pub fn operator(principal: Value) -> Self {
        Self::Operator { principal }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SenderView {
    Session {
        session_id: Uuid,
        role: String,
        display_label: String,
        labels: Vec<Label>,
        namespace: Namespace,
    },
    Operator {
        principal: Value,
        display_label: String,
    },
    System,
}

impl SenderView {
    pub fn session(session: &Session) -> Self {
        Self::Session {
            session_id: session.id,
            role: session.role.clone(),
            display_label: session_display_label(session),
            labels: session.labels.clone(),
            namespace: session.namespace.clone(),
        }
    }

    pub fn operator(principal: Value) -> Self {
        Self::Operator {
            principal,
            display_label: "operator".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecipientSummary {
    pub session_id: Uuid,
    pub role: String,
    pub display_label: String,
    pub namespace: Namespace,
}

impl RecipientSummary {
    pub fn from_session(session: &Session) -> Self {
        Self {
            session_id: session.id,
            role: session.role.clone(),
            display_label: session_display_label(session),
            namespace: session.namespace.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageView {
    pub id: Uuid,
    pub content: String,
    pub sent_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    pub status: MailStatus,
    pub sender: SenderView,
    pub recipient: RecipientSummary,
    pub context_id: String,
    pub intent: MailIntent,
}

impl MessageView {
    pub fn from_mail(mail: &Mail, sender: SenderView, recipient: RecipientSummary) -> Self {
        Self {
            id: mail.id,
            content: mail.content.clone(),
            sent_at: mail.sent_at,
            read_at: mail.read_at,
            status: mail.status,
            sender,
            recipient,
            context_id: mail.context_id.clone(),
            intent: mail.intent,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailDeliveryStatus {
    Ok,
    Err,
}

impl fmt::Display for MailDeliveryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => f.write_str("ok"),
            Self::Err => f.write_str("err"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailNotifyStatus {
    Ok,
    Err,
    Skipped,
}

impl fmt::Display for MailNotifyStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => f.write_str("ok"),
            Self::Err => f.write_str("err"),
            Self::Skipped => f.write_str("skipped"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailSendResult {
    pub recipient: RecipientSummary,
    pub mail: MailDeliveryStatus,
    pub notify: MailNotifyStatus,
    pub message: Option<MessageView>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailCountView {
    pub session_id: Uuid,
    pub role: String,
    pub display_label: String,
    pub namespace: Namespace,
    pub unread: usize,
}

impl MailCountView {
    pub fn from_session(session: &Session, unread: usize) -> Self {
        Self {
            session_id: session.id,
            role: session.role.clone(),
            display_label: session_display_label(session),
            namespace: session.namespace.clone(),
            unread,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mail {
    pub id: Uuid,
    pub sender: SenderRef,
    pub recipient_id: Uuid,
    pub content: String,
    pub sent_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    /// Authoritative delivery state. Persisted in `message_deliveries.status`;
    /// not derived from `read_at`, so it can carry `Undeliverable`.
    pub status: MailStatus,
    pub context_id: String,
    pub intent: MailIntent,
    pub idempotency_key: Option<String>,
}

fn session_display_label(session: &Session) -> String {
    session.role.clone()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Mail,
    Nudge,
}
