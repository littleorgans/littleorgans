use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::label::Label;
use crate::namespace::Namespace;
use crate::runtime::RuntimeKind;
use crate::{SmError, SmResult};

pub use lilo_rm_core::LostEvidence;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionState {
    Spawning,
    Running,
    Terminated,
    Lost { evidence: LostEvidence },
}

impl fmt::Display for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawning => f.write_str("SPAWNING"),
            Self::Running => f.write_str("RUNNING"),
            Self::Terminated => f.write_str("TERMINATED"),
            Self::Lost { evidence } => write!(f, "Lost({evidence})"),
        }
    }
}

impl SessionState {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Spawning | Self::Running)
    }

    pub fn sql_name(&self) -> &'static str {
        match self {
            Self::Spawning => "SPAWNING",
            Self::Running => "RUNNING",
            Self::Terminated => "TERMINATED",
            Self::Lost { .. } => "LOST",
        }
    }

    pub fn from_sql(value: &str, lost_evidence: Option<LostEvidence>) -> SmResult<Self> {
        match value {
            "SPAWNING" => Ok(Self::Spawning),
            "RUNNING" => Ok(Self::Running),
            "TERMINATED" => Ok(Self::Terminated),
            "LOST" => Ok(Self::Lost {
                evidence: lost_evidence.ok_or_else(|| {
                    SmError::Message("lost session missing lost_evidence".to_string())
                })?,
            }),
            other => Err(SmError::Message(format!(
                "unsupported session state: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub id: Uuid,
    pub runtime: RuntimeKind,
    pub role: String,
    pub workspace: String,
    #[serde(default)]
    pub namespace: Namespace,
    #[serde(default)]
    pub dir: PathBuf,
    #[serde(default)]
    pub labels: Vec<Label>,
    pub state: SessionState,
    pub runtime_pid: u32,
    pub runtime_session: Option<String>,
    pub transcript_path: Option<PathBuf>,
    pub tmux_pane: Option<String>,
    pub agent_config: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub terminated_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub updated_at: DateTime<Utc>,
}
