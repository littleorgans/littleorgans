use lilo_common::id::SessionId;
use lilo_port::PortError;
pub use lilo_rm_core::LaunchEnv;
use lilo_rm_core::{
    CaptureResponse, IsolationPolicy, LaunchAttachment, Lifecycle, MountSpec, ShellResume,
    SpawnConflictKind, SpawnTarget,
};
use lilo_session_core::RuntimeKind;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnedProcess {
    pub lifecycle: Lifecycle,
    pub runtime_pid: u32,
    pub log_dir: Option<PathBuf>,
    pub stdout_path: Option<PathBuf>,
    pub stderr_path: Option<PathBuf>,
    pub tmux_pane: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnLaunch {
    pub session_id: SessionId,
    pub runtime: RuntimeKind,
    pub isolation: IsolationPolicy,
    pub image: Option<String>,
    pub cwd: PathBuf,
    pub target: SpawnTarget,
    pub env: Vec<LaunchEnv>,
    pub mounts: Vec<MountSpec>,
    pub shell_resume: Option<ShellResume>,
    pub force: bool,
    pub launch_attachment: Option<LaunchAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildExit {
    pub session_id: SessionId,
    pub runtime_pid: u32,
    pub exit_code: Option<i32>,
    pub transcript_path: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum RuntimeFault {
    #[error("{message}")]
    SpawnConflict {
        kind: SpawnConflictKind,
        message: String,
    },
}

pub type RuntimeError = PortError<RuntimeFault>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NudgeResult {
    pub delivered: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureResult {
    pub response: CaptureResponse,
}
