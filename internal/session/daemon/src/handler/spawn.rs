use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use lilo_db::{begin_immediate_tx, finish_immediate_tx};
use lilo_im_core::Action;
use lilo_rm_core::{
    LaunchEnv, Lifecycle, LifecycleState, RuntimeEvent, ShellResume, StatusFilter,
    capture_caller_env, capture_shell_resume,
};
use lilo_runtime_store::LifecycleStore;
use lilo_session_core::{RpcResponse, Session, SessionState, SpawnRequest, SpawnResponse};
use lilo_session_driver::{DriverError, SpawnLaunch, runtime_spawn_request};
use lilo_session_store::{PendingSpawnIntent, SessionDraft, SessionSpawnIntent};
use sqlx::{Sqlite, pool::PoolConnection};
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
            let store = self.store();
            normalize_spawn_request(&mut request, store).await?
        };
        let agent_config = resolve_agent_config(request.agent_config.as_deref())?;
        let agent_config_path = agent_config
            .as_ref()
            .map(|config| config.path.display().to_string());
        let launch = spawn_launch(id, &request, agent_config.as_ref());
        let mut labels = request.labels.clone();
        labels.sort();
        let runtime_request =
            runtime_spawn_request(id, &launch).context("failed to build runtime spawn request")?;
        let draft_created_at = Utc::now();
        let draft_session = Session {
            id,
            runtime: request.runtime,
            role: request.role.clone(),
            workspace: request.workspace.clone(),
            namespace: location.namespace.clone(),
            dir: location.dir.clone(),
            labels,
            state: SessionState::Running,
            runtime_pid: 0,
            runtime_session: None,
            transcript_path: None,
            tmux_pane: None,
            agent_config: agent_config_path,
            created_at: draft_created_at,
            started_at: draft_created_at,
            terminated_at: None,
            exit_code: None,
            updated_at: draft_created_at,
        };
        let intent = PendingSpawnIntent::new(
            Uuid::now_v7(),
            runtime_request.clone(),
            SessionDraft::new(&draft_session),
        );

        self.begin_spawn_intent(context, &request, &intent).await?;
        let id_string = id.to_string();
        let spawned = match self.runtime.spawn(&id_string, &launch).await {
            Ok(spawned) => spawned,
            Err(error) => {
                let failure = runtime_spawn_failure(&error);
                self.abort_spawn_intent(id, &failure).await?;
                anyhow::bail!("runtime spawn failed: {failure}");
            }
        };
        let event = running_event_from_lifecycle(&spawned.lifecycle)?;
        let session = self
            .complete_spawn_intent(
                &intent,
                spawned.lifecycle,
                event,
                spawned.stdout_path,
                OnCommitFailure::AbortRunning,
            )
            .await?;

        Ok(RpcResponse::Spawned {
            response: SpawnResponse { session },
        })
    }

    async fn begin_spawn_intent(
        &self,
        context: &RequestContext,
        request: &SpawnRequest,
        intent: &PendingSpawnIntent,
    ) -> Result<()> {
        let lifecycle_store = self.lifecycle_store();
        let mut conn = self
            .begin_spawn_tx(
                "session spawn Tx A",
                "failed to acquire session spawn Tx A connection",
            )
            .await?;
        if let Err(error) = self
            .identity
            .authorize_in_tx(
                &mut conn,
                &context.principal,
                Action::Spawn,
                &spawn_resource(request, intent.session_id),
            )
            .await
        {
            finish_immediate_tx(&mut conn, Ok(()), "session spawn Tx A").await?;
            return Err(error);
        }
        let result = async {
            self.store
                .insert_pending_spawn_intent_in(&mut conn, intent)
                .await
                .context("failed to insert pending spawn intent")?;
            let mut lifecycle =
                Lifecycle::forking(intent.session_id, intent.spawn_request.runtime.clone());
            lifecycle.isolation = intent.spawn_request.isolation.clone();
            lifecycle_store
                .insert_forking_in(&mut conn, &lifecycle)
                .await
                .context("failed to insert Forking lifecycle")?;
            Ok(())
        }
        .await;
        finish_immediate_tx(&mut conn, result, "session spawn Tx A").await
    }

    async fn complete_spawn_intent(
        &self,
        intent: &PendingSpawnIntent,
        lifecycle: Lifecycle,
        event: RuntimeEvent,
        stdout_path: Option<std::path::PathBuf>,
        on_commit_failure: OnCommitFailure,
    ) -> Result<Session> {
        let updated_at = Utc::now();
        let session = intent
            .session_draft
            .running_session(&lifecycle, stdout_path, updated_at)
            .context("failed to build session from runtime lifecycle")?;
        if !self
            .store
            .namespace_exists(&session.namespace)
            .await
            .context("failed to revalidate namespace before session commit")?
        {
            self.abort_running_spawn(intent.session_id, "namespace deleted before session commit")
                .await?;
            anyhow::bail!(
                "namespace deleted before session commit: {}",
                session.namespace
            );
        }

        let lifecycle_store = self.lifecycle_store();
        let mut conn = self
            .begin_spawn_tx(
                "session spawn Tx B",
                "failed to acquire session spawn Tx B connection",
            )
            .await?;
        let result = async {
            self.store
                .insert_session_in(&mut conn, &session)
                .await
                .context("failed to persist session")?;
            lifecycle_store
                .update_lifecycle_in(&mut conn, &lifecycle)
                .await
                .context("failed to persist Running lifecycle")?;
            self.store
                .resolve_spawn_intent_in(&mut conn, intent.session_id)
                .await
                .context("failed to resolve spawn intent")?;
            Ok(())
        }
        .await;
        if let Err(error) = finish_immediate_tx(&mut conn, result, "session spawn Tx B").await {
            match on_commit_failure {
                OnCommitFailure::AbortRunning => {
                    let abort_reason = format!("session commit failed: {error}");
                    if let Err(abort_error) = self
                        .abort_running_spawn(intent.session_id, &abort_reason)
                        .await
                    {
                        tracing::warn!(
                            error = %abort_error,
                            session_id = %intent.session_id,
                            "failed to abort running spawn after session commit failure"
                        );
                    }
                }
                OnCommitFailure::LeavePending => {}
            }
            return Err(error);
        }

        self.runtime_service
            .append_event(event)
            .await
            .context("failed to append runtime event after session commit")?;
        Ok(session)
    }

    async fn abort_running_spawn(&self, session_id: Uuid, reason: &str) -> Result<()> {
        let session_id_string = session_id.to_string();
        match self
            .runtime
            .terminate(&session_id_string, "SIGTERM", Duration::from_secs(5))
            .await
        {
            Ok(Some(_)) => {}
            Ok(None) => {
                tracing::warn!(
                    session_id = %session_id,
                    "recovery kill did not observe orphaned runtime process exit"
                );
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    session_id = %session_id,
                    "recovery kill of orphaned runtime process failed"
                );
            }
        }
        self.abort_spawn_intent(session_id, reason).await
    }

    async fn abort_spawn_intent(&self, session_id: Uuid, reason: &str) -> Result<()> {
        let lifecycle_store = self.lifecycle_store();
        let mut conn = self
            .begin_spawn_tx(
                "session spawn abort tx",
                "failed to acquire session spawn abort tx connection",
            )
            .await?;
        let result = async {
            self.store
                .abort_spawn_intent_in(&mut conn, session_id, reason)
                .await
                .context("failed to abort spawn intent")?;
            lifecycle_store
                .delete_in(&mut conn, session_id)
                .await
                .context("failed to delete aborted lifecycle")?;
            Ok(())
        }
        .await;
        finish_immediate_tx(&mut conn, result, "session spawn abort tx").await
    }

    pub(crate) async fn reconcile_pending_spawn_intents(&self) -> Result<()> {
        let pending = self
            .store
            .list_pending_spawn_intents()
            .await
            .context("failed to list pending spawn intents")?;
        for intent in pending {
            let session_id = intent.session_id;
            if let Err(error) = self.reconcile_pending_spawn_intent(intent).await {
                tracing::warn!(
                    error = ?error,
                    session_id = %session_id,
                    "failed to reconcile pending spawn intent; continuing"
                );
            }
        }
        Ok(())
    }

    async fn reconcile_pending_spawn_intent(&self, intent: SessionSpawnIntent) -> Result<()> {
        let status = match self
            .runtime
            .status(StatusFilter::for_session(intent.session_id))
            .await
        {
            Ok(lifecycles) => lifecycles,
            Err(error) => {
                let failure = runtime_spawn_failure(&error);
                self.abort_spawn_intent(intent.session_id, &failure).await?;
                tracing::warn!(
                    error = %error,
                    session_id = %intent.session_id,
                    "runtime status failed during spawn intent reconcile"
                );
                return Ok(());
            }
        };
        let Some(lifecycle) = status
            .into_iter()
            .find(|lifecycle| lifecycle.session_id == intent.session_id)
        else {
            self.abort_spawn_intent(intent.session_id, "runtime lifecycle missing")
                .await?;
            return Ok(());
        };
        if lifecycle.state != LifecycleState::Running {
            self.abort_spawn_intent(intent.session_id, "runtime lifecycle not running")
                .await?;
            return Ok(());
        }
        let event = running_event_from_lifecycle(&lifecycle)?;
        let pending = PendingSpawnIntent {
            session_id: intent.session_id,
            operation_id: intent.operation_id,
            spawn_request: intent.spawn_request,
            session_draft: intent.session_draft,
            created_at: intent.created_at,
        };
        self.complete_spawn_intent(
            &pending,
            lifecycle,
            event,
            None,
            OnCommitFailure::LeavePending,
        )
        .await?;
        Ok(())
    }

    fn lifecycle_store(&self) -> LifecycleStore {
        LifecycleStore::from_pool(self.store.pool().clone())
    }

    async fn begin_spawn_tx(
        &self,
        label: &'static str,
        acquire_context: &'static str,
    ) -> Result<PoolConnection<Sqlite>> {
        let mut conn = self.store.pool().acquire().await.context(acquire_context)?;
        begin_immediate_tx(&mut conn, label).await?;
        Ok(conn)
    }
}

#[derive(Clone, Copy)]
enum OnCommitFailure {
    AbortRunning,
    LeavePending,
}

fn running_event_from_lifecycle(lifecycle: &Lifecycle) -> Result<RuntimeEvent> {
    let LifecycleState::Running = lifecycle.state else {
        anyhow::bail!("lifecycle {} is not running", lifecycle.session_id);
    };
    let runtime_pid = lifecycle
        .runtime_pid
        .context("running lifecycle missing runtime pid")?;
    let start_time = lifecycle.start_time.unwrap_or_else(Utc::now);
    Ok(RuntimeEvent::Running {
        session_id: lifecycle.session_id,
        runtime_pid,
        start_time,
    })
}

fn runtime_spawn_failure(error: &DriverError) -> String {
    match error {
        DriverError::SpawnConflict { kind, .. } => {
            format!("spawn conflict: {kind:?}")
        }
        other => other.to_string(),
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

#[cfg(test)]
mod tests;
