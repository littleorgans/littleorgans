use std::sync::OnceLock;

use lilo_rm_core::{LaunchEnv, LauncherError, RuntimeKind, RuntimeLauncher, SpawnRequest};

static CLAUDE_PATH: OnceLock<Result<String, LauncherError>> = OnceLock::new();

pub struct ClaudeLauncher;

impl RuntimeLauncher for ClaudeLauncher {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Claude
    }

    fn argv(&self, _request: &SpawnRequest) -> Result<Vec<String>, LauncherError> {
        crate::resolved_argv("claude", &CLAUDE_PATH)
    }

    fn env(&self, request: &SpawnRequest) -> Result<Vec<LaunchEnv>, LauncherError> {
        Ok(crate::runtime_env(request))
    }
}
