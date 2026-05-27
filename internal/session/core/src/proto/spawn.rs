use lilo_rm_core::{IsolationPolicy, LaunchEnv, MountSpec, ShellResume};
use serde::{Deserialize, Serialize};

use crate::{Namespace, RuntimeKind, Selector, Session};

fn default_spawn_target() -> String {
    "headless".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnRequest {
    pub runtime: RuntimeKind,
    pub role: String,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub namespace: Option<Namespace>,
    #[serde(default = "default_spawn_target")]
    pub target: String,
    #[serde(default)]
    pub agent_config: Option<String>,
    #[serde(default)]
    pub isolation: IsolationPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default)]
    pub env: Vec<LaunchEnv>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<MountSpec>,
    #[serde(default)]
    pub shell_resume: Option<ShellResume>,
    #[serde(default)]
    pub labels: Vec<crate::Label>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnResponse {
    pub session: Session,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ListRequest {
    pub selector: Option<Selector>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListResponse {
    pub sessions: Vec<Session>,
}
