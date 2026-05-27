use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::TargetError;
use crate::{LabelMutation, Selector, Session, SmError, SmResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeleteRequest {
    pub selector: Selector,
    pub signal: String,
    pub grace_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeleteResponse {
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub errors: Vec<TargetError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LabelRequest {
    pub selector: Selector,
    pub mutation: LabelMutation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LabelResponse {
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub errors: Vec<TargetError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogsRequest {
    pub selector: Selector,
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogsResponse {
    pub session: Session,
    pub transcript_path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureRequest {
    pub session_id: uuid::Uuid,
    #[serde(default)]
    pub scrollback_lines: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureResponse {
    pub session: Session,
    pub capture: lilo_rm_core::CaptureResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaitRequest {
    pub selector: Selector,
    pub condition: WaitCondition,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WaitCondition {
    Running,
    Terminated,
    Count { count: usize },
}

impl FromStr for WaitCondition {
    type Err = SmError;

    fn from_str(value: &str) -> SmResult<Self> {
        match value {
            "running" => Ok(Self::Running),
            "terminated" => Ok(Self::Terminated),
            raw => {
                let Some(count) = raw.strip_prefix("count=") else {
                    return Err(SmError::Message(format!(
                        "unsupported wait condition: {raw}"
                    )));
                };
                Ok(Self::Count {
                    count: count
                        .parse()
                        .map_err(|_| SmError::Message(format!("invalid wait count: {count}")))?,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaitResponse {
    pub matched: bool,
    pub sessions: Vec<Session>,
}
