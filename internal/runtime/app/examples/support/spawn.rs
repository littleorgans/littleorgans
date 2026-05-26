use std::path::Path;

use anyhow::{Context, Result};
use lilo_rm_core::{
    IsolationPolicy, RuntimeKind, RuntimeResponse, RuntimeRpc, SpawnRequest, SpawnTarget,
};
use uuid::Uuid;

pub async fn spawn_runtime(
    socket_path: &Path,
    session_id: Uuid,
    runtime: RuntimeKind,
    target: SpawnTarget,
) -> Result<RuntimeResponse> {
    let cwd = lilo_rm_core::capture_caller_cwd().context("failed to capture caller cwd")?;
    let env = lilo_rm_core::capture_caller_env();
    lilo_runtime_app::shared::request(
        socket_path,
        RuntimeRpc::Spawn {
            request: SpawnRequest {
                session_id,
                runtime,
                isolation: IsolationPolicy::default(),
                image: None,
                env,
                mounts: Vec::new(),
                cwd,
                target,
                force: false,
                shell_resume: None,
            },
        },
    )
    .await
}
