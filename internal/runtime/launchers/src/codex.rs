use std::sync::OnceLock;

use lilo_rm_core::{LaunchEnv, LauncherError, RuntimeKind, RuntimeLauncher, SpawnRequest};

static CODEX_PATH: OnceLock<Result<String, LauncherError>> = OnceLock::new();

pub struct CodexLauncher;

impl RuntimeLauncher for CodexLauncher {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Codex
    }

    fn argv(&self, _request: &SpawnRequest) -> Result<Vec<String>, LauncherError> {
        crate::resolved_argv("codex", &CODEX_PATH)
    }

    fn env(&self, request: &SpawnRequest) -> Result<Vec<LaunchEnv>, LauncherError> {
        Ok(crate::runtime_env(request))
    }
}
