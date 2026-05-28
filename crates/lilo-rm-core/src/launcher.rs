use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{RuntimeKind, SpawnRequest};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LaunchEnv {
    pub key: String,
    pub value: String,
}

impl LaunchEnv {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

pub fn upsert_launch_env(env: &mut Vec<LaunchEnv>, next: LaunchEnv) {
    if let Some(existing) = env.iter_mut().find(|entry| entry.key == next.key) {
        *existing = next;
    } else {
        env.push(next);
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LaunchSpec {
    pub argv: Vec<String>,
    pub env: Vec<LaunchEnv>,
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_resume: Option<ShellResume>,
}

impl LaunchSpec {
    pub fn command(&self) -> Result<&str, LauncherError> {
        argv_command(&self.argv, LauncherError::EmptyArgv)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShellResume {
    pub argv: Vec<String>,
    pub env: Vec<LaunchEnv>,
    pub cwd: PathBuf,
}

impl ShellResume {
    pub fn command(&self) -> Result<&str, LauncherError> {
        argv_command(&self.argv, LauncherError::EmptyShellArgv)
    }
}

fn argv_command(argv: &[String], empty_error: LauncherError) -> Result<&str, LauncherError> {
    argv.first().map(String::as_str).ok_or(empty_error)
}

pub trait RuntimeLauncher: Sync {
    fn kind(&self) -> RuntimeKind;

    fn argv(&self, request: &SpawnRequest) -> Result<Vec<String>, LauncherError>;

    fn env(&self, request: &SpawnRequest) -> Result<Vec<LaunchEnv>, LauncherError>;

    fn cwd(&self, request: &SpawnRequest) -> Result<PathBuf, LauncherError> {
        Ok(request.cwd.clone())
    }

    fn launch_spec(&self, request: &SpawnRequest) -> Result<LaunchSpec, LauncherError> {
        let spec = LaunchSpec {
            argv: self.argv(request)?,
            env: self.env(request)?,
            cwd: self.cwd(request)?,
            shell_resume: request.shell_resume.clone(),
        };
        if spec.argv.is_empty() {
            return Err(LauncherError::EmptyArgv);
        }
        if spec.env.is_empty() {
            return Err(LauncherError::EmptyEnv {
                runtime_kind: self.kind().to_string(),
            });
        }
        Ok(spec)
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum LauncherError {
    #[error("no launcher registered for runtime kind: {runtime_kind}")]
    NoLauncher { runtime_kind: String },
    #[error("launcher produced empty argv")]
    EmptyArgv,
    #[error("launcher produced empty shell resume argv")]
    EmptyShellArgv,
    #[error("launcher {runtime_kind} produced empty env")]
    EmptyEnv { runtime_kind: String },
    #[error("failed to resolve launcher binary {binary}: {message}")]
    BinaryLookupFailed { binary: String, message: String },
}
