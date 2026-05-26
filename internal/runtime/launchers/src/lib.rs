#![forbid(unsafe_code)]

//! Runtime command registry for known agent launchers.
//!
//! The daemon asks this crate for Claude and Codex launch specs while keeping
//! spawn orchestration independent from runtime command details.

mod claude;
mod codex;

use std::sync::OnceLock;

pub use claude::ClaudeLauncher;
pub use codex::CodexLauncher;
use lilo_rm_core::{
    HeadlessSpawnTarget, IsolationPolicy, LaunchEnv, LauncherError, RuntimeKind, RuntimeLauncher,
    SpawnRequest, SpawnTarget, upsert_launch_env,
};

static CLAUDE: ClaudeLauncher = ClaudeLauncher;
static CODEX: CodexLauncher = CodexLauncher;

pub fn dispatch(kind: &RuntimeKind) -> Result<&'static dyn RuntimeLauncher, LauncherError> {
    match kind {
        RuntimeKind::Claude => Ok(&CLAUDE),
        RuntimeKind::Codex => Ok(&CODEX),
        RuntimeKind::Other(value) => Err(LauncherError::NoLauncher {
            runtime_kind: value.clone(),
        }),
    }
}

pub fn registered_launchers() -> [&'static dyn RuntimeLauncher; 2] {
    [&CLAUDE, &CODEX]
}

pub fn warm_registry() -> Result<(), LauncherError> {
    for launcher in registered_launchers() {
        let request = SpawnRequest {
            session_id: uuid::Uuid::nil(),
            runtime: launcher.kind(),
            isolation: IsolationPolicy::default(),
            image: None,
            env: Vec::new(),
            mounts: Vec::new(),
            cwd: lilo_rm_core::launcher_probe_cwd(),
            target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
            force: false,
            shell_resume: None,
        };
        launcher.argv(&request)?;
    }
    Ok(())
}

pub(crate) fn resolved_argv(
    binary: &'static str,
    cache: &OnceLock<Result<String, LauncherError>>,
) -> Result<Vec<String>, LauncherError> {
    Ok(vec![cached_binary(binary, cache)?])
}

pub(crate) fn runtime_env(request: &SpawnRequest) -> Vec<LaunchEnv> {
    let mut env = request.env.clone();
    upsert_launch_env(
        &mut env,
        LaunchEnv::new("HELIOY_SESSION_ID", request.session_id.to_string()),
    );
    upsert_launch_env(
        &mut env,
        LaunchEnv::new("HELIOY_RUNTIME", request.runtime.to_string()),
    );
    upsert_launch_env(
        &mut env,
        LaunchEnv::new("RTM_SESSION_ID", request.session_id.to_string()),
    );
    upsert_launch_env(
        &mut env,
        LaunchEnv::new("RTM_RUNTIME_KIND", request.runtime.to_string()),
    );
    env
}

fn cached_binary(
    binary: &'static str,
    cache: &OnceLock<Result<String, LauncherError>>,
) -> Result<String, LauncherError> {
    cache.get_or_init(|| resolve_binary(binary)).clone()
}

fn resolve_binary(binary: &'static str) -> Result<String, LauncherError> {
    let output = std::process::Command::new("which")
        .arg(binary)
        .output()
        .map_err(|error| LauncherError::BinaryLookupFailed {
            binary: binary.to_owned(),
            message: error.to_string(),
        })?;
    if !output.status.success() {
        return Ok(binary.to_owned());
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if path.is_empty() {
        Ok(binary.to_owned())
    } else {
        Ok(path)
    }
}
