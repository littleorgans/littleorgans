use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{SmError, SmResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailStatus {
    Unread,
    Read,
}

impl fmt::Display for MailStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unread => f.write_str("unread"),
            Self::Read => f.write_str("read"),
        }
    }
}

impl FromStr for MailStatus {
    type Err = SmError;

    fn from_str(value: &str) -> SmResult<Self> {
        match value {
            "unread" => Ok(Self::Unread),
            "read" => Ok(Self::Read),
            other => Err(SmError::Message(format!(
                "unsupported mail status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mail {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub recipient_id: Uuid,
    pub content: String,
    pub sent_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

impl Mail {
    pub fn status(&self) -> MailStatus {
        if self.read_at.is_some() {
            MailStatus::Read
        } else {
            MailStatus::Unread
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Mail,
    Nudge,
}
