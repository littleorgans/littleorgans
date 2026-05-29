use lilo_port::PortError;
pub use lilo_rm_core::LaunchEnv;
use lilo_rm_core::{
    CaptureResponse, IsolationPolicy, Lifecycle, MountSpec, ShellResume, SpawnConflictKind,
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
    pub runtime: RuntimeKind,
    pub isolation: IsolationPolicy,
    pub image: Option<String>,
    pub cwd: PathBuf,
    pub target: String,
    pub env: Vec<LaunchEnv>,
    pub mounts: Vec<MountSpec>,
    pub shell_resume: Option<ShellResume>,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildExit {
    pub session_id: String,
    pub runtime_pid: u32,
    pub exit_code: Option<i32>,
    pub transcript_path: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum RuntimeFault {
    #[error("unsupported signal: {0}")]
    InvalidSignal(String),
    #[error("invalid runtime session id: {0}")]
    InvalidSessionId(String),
    #[error("{message}")]
    SpawnConflict {
        kind: SpawnConflictKind,
        message: String,
    },
    #[error("invalid runtime target: {0}")]
    InvalidTarget(String),
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
