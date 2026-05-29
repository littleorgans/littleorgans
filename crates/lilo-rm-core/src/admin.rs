use serde::{Deserialize, Serialize};
use uuid::Uuid;

use chrono::{DateTime, Utc};

use crate::{Lifecycle, LogAvailability, LostEvidence, VersionInfo};

/// Result of a kill request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum KillOutcome {
    /// A signal was delivered to the target process.
    Signalled,
    /// The target process had already exited before the signal landed.
    AlreadyExited,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KillByPidRequest {
    pub pid: u32,
    pub signal: i32,
    pub grace_secs: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KillByPidResponse {
    pub pid: u32,
    pub signal: i32,
    pub killed_after_grace: bool,
    pub outcome: KillOutcome,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusFilter {
    pub session_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_since: Option<DateTime<Utc>>,
    pub runtime: Option<String>,
    pub state: Option<String>,
}

impl StatusFilter {
    pub const fn empty() -> Self {
        Self {
            session_id: None,
            session_ids: Vec::new(),
            updated_since: None,
            runtime: None,
            state: None,
        }
    }

    pub const fn for_session(session_id: Uuid) -> Self {
        Self {
            session_id: Some(session_id),
            session_ids: Vec::new(),
            updated_since: None,
            runtime: None,
            state: None,
        }
    }

    pub fn requested_session_ids(&self) -> Vec<Uuid> {
        let mut ids = self.session_ids.clone();
        if let Some(session_id) = self.session_id
            && !ids.contains(&session_id)
        {
            ids.push(session_id);
        }
        ids
    }
}

impl Default for StatusFilter {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusResponse {
    pub lifecycles: Vec<Lifecycle>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WatcherCounts {
    pub process_exit_watchers: usize,
    pub shim_sockets: usize,
    pub event_waiters: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LifecycleCounts {
    pub forking: u64,
    pub running: u64,
    pub exited: u64,
    pub lost: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigrationState {
    pub applied: usize,
    pub total: usize,
    pub applied_descriptions: Vec<String>,
    pub pending_descriptions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecentLostEvent {
    pub session_id: Uuid,
    pub evidence: LostEvidence,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LauncherStatus {
    pub runtime: String,
    pub command: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmuxStatus {
    pub available: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DockerReadiness {
    pub ready: bool,
    pub detail: Option<String>,
    pub error: Option<String>,
}

impl DockerReadiness {
    pub fn ready(detail: impl Into<String>) -> Self {
        Self {
            ready: true,
            detail: Some(detail.into()),
            error: None,
        }
    }

    pub fn unavailable(error: impl Into<String>) -> Self {
        Self {
            ready: false,
            detail: None,
            error: Some(error.into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DockerIsolationStatus {
    pub supported: bool,
    pub default_workspace: String,
    pub experimental: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DockerStatus {
    pub cli: DockerReadiness,
    pub daemon: DockerReadiness,
    pub manifest_validation: DockerReadiness,
    pub isolation: DockerIsolationStatus,
}

impl DockerStatus {
    pub fn legacy_missing() -> Self {
        Self {
            cli: DockerReadiness::unavailable("not reported by this daemon"),
            daemon: DockerReadiness::unavailable("not reported by this daemon"),
            manifest_validation: DockerReadiness::unavailable("not reported by this daemon"),
            isolation: DockerIsolationStatus {
                supported: false,
                default_workspace: String::new(),
                experimental: false,
            },
        }
    }

    pub fn is_legacy_missing(&self) -> bool {
        self == &Self::legacy_missing()
    }
}

fn legacy_missing_docker_status() -> Box<DockerStatus> {
    Box::new(DockerStatus::legacy_missing())
}

fn is_legacy_missing_docker_status(status: &DockerStatus) -> bool {
    status.is_legacy_missing()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LifecycleLogAvailability {
    pub session_id: Uuid,
    pub log_availability: LogAvailability,
}

/// Stable v0.4 daemon diagnostics JSON.
///
/// Clients may rely on the field names and JSON value kinds in this response.
/// The concrete diagnostic values are host and process specific.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DoctorResponse {
    pub version: VersionInfo,
    pub socket_path: String,
    pub uptime_secs: u64,
    pub sqlite: MigrationState,
    pub lifecycles: LifecycleCounts,
    pub watchers: WatcherCounts,
    pub launchers: Vec<LauncherStatus>,
    pub tmux: TmuxStatus,
    #[serde(
        default = "legacy_missing_docker_status",
        skip_serializing_if = "is_legacy_missing_docker_status"
    )]
    pub docker: Box<DockerStatus>,
    pub log_availability: Vec<LifecycleLogAvailability>,
    pub last_probe_sweep: Option<DateTime<Utc>>,
    pub recent_lost: Vec<RecentLostEvent>,
}
