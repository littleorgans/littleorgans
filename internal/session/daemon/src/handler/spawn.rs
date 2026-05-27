use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use lilo_im_core::Action;
use lilo_rm_core::{LaunchEnv, ShellResume, capture_caller_env, capture_shell_resume};
use lilo_session_core::{RpcResponse, Session, SessionState, SpawnRequest, SpawnResponse};
use lilo_session_driver::SpawnLaunch;
use uuid::Uuid;

use crate::agent_config::{ResolvedAgentConfig, resolve_agent_config};
use crate::identity_client::{RequestContext, spawn_resource};
use crate::spawn_request::normalize_spawn_request;

use super::DaemonState;

impl DaemonState {
    pub(super) async fn spawn(
        &self,
        context: &RequestContext,
        mut request: SpawnRequest,
    ) -> Result<RpcResponse> {
        let id = Uuid::now_v7();
        let location = {
            let store = self.store()?;
            normalize_spawn_request(&mut request, &store)?
        };
        let agent_config = resolve_agent_config(request.agent_config.as_deref())?;
        let agent_config_path = agent_config
            .as_ref()
            .map(|config| config.path.display().to_string());
        let launch = spawn_launch(id, &request, agent_config.as_ref());
        let mut labels = request.labels.clone();
        labels.sort();
        self.identity
            .authorize(
                &context.principal,
                Action::Spawn,
                &spawn_resource(&request, id),
            )
            .await?;
        self.driver
            .validate_target(&request.target)
            .await
            .context("runtime target validation failed")?;
        let spawned = self
            .driver
            .spawn(&id.to_string(), &launch)
            .await
            .context("spawn driver failed")?;
        let now = Utc::now();
        let session = Session {
            id,
            runtime: request.runtime,
            role: request.role,
            workspace: request.workspace,
            namespace: location.namespace,
            dir: location.dir,
            labels,
            state: SessionState::Running,
            runtime_pid: spawned.runtime_pid,
            runtime_session: None,
            transcript_path: spawned.stdout_path,
            tmux_pane: spawned.tmux_pane,
            agent_config: agent_config_path,
            created_at: now,
            started_at: now,
            terminated_at: None,
            exit_code: None,
            updated_at: now,
        };

        let namespace_deleted_before_commit = {
            let store = self.store()?;
            if store
                .namespace_exists(&session.namespace)
                .context("failed to revalidate namespace before session commit")?
            {
                store
                    .insert_session(&session)
                    .context("failed to persist session")?;
                false
            } else {
                true
            }
        };
        if namespace_deleted_before_commit {
            let _ = self
                .driver
                .terminate(&id.to_string(), "SIGTERM", Duration::from_secs(5))
                .await;
            anyhow::bail!(
                "namespace deleted before session commit: {}",
                session.namespace
            );
        }

        Ok(RpcResponse::Spawned {
            response: SpawnResponse { session },
        })
    }
}

fn spawn_launch(
    id: Uuid,
    request: &SpawnRequest,
    agent_config: Option<&ResolvedAgentConfig>,
) -> SpawnLaunch {
    let mut env = request.env.clone();
    if env.is_empty() {
        env = capture_caller_env();
    }
    if let Some(config) = agent_config {
        merge_env(&mut env, config.env.clone());
    }
    env.retain(|item| !item.key.starts_with("HELIOY_SESSION_"));
    upsert_env(
        &mut env,
        LaunchEnv::new("HELIOY_SESSION_ID", id.to_string()),
    );
    upsert_env(
        &mut env,
        LaunchEnv::new("HELIOY_SESSION_ROLE", request.role.clone()),
    );
    upsert_env(
        &mut env,
        LaunchEnv::new("HELIOY_SESSION_WORKSPACE", request.workspace.clone()),
    );
    let cwd = std::path::PathBuf::from(&request.workspace);
    let shell_resume = shell_resume(request, &cwd);
    SpawnLaunch {
        runtime: request.runtime,
        isolation: request.isolation.clone(),
        image: request.image.clone(),
        cwd,
        target: request.target.clone(),
        env,
        mounts: request.mounts.clone(),
        shell_resume,
        force: request.force,
    }
}

fn shell_resume(request: &SpawnRequest, cwd: &std::path::Path) -> Option<ShellResume> {
    if request.shell_resume.is_some() {
        return request.shell_resume.clone();
    }
    request
        .target
        .parse::<lilo_rm_core::SpawnTarget>()
        .ok()
        .and_then(|target| {
            target
                .tmux_address()
                .map(|_| capture_shell_resume(cwd.to_path_buf()))
        })
}

fn merge_env(env: &mut Vec<LaunchEnv>, next: Vec<LaunchEnv>) {
    for item in next {
        upsert_env(env, item);
    }
}

fn upsert_env(env: &mut Vec<LaunchEnv>, next: LaunchEnv) {
    if let Some(existing) = env.iter_mut().find(|item| item.key == next.key) {
        *existing = next;
    } else {
        env.push(next);
    }
}
