use anyhow::{Context, Result};
use chrono::Utc;
use lilo_im_core::{Action, Principal};
use lilo_rm_core::{
    KillRequest, LaunchEnv, Lifecycle, LifecycleState, RuntimeEvent, RuntimeResponse, RuntimeRpc,
    RuntimeSignal, ShellResume, StatusRequest, capture_caller_env, capture_shell_resume,
};
use lilo_runtime_store::LifecycleStore;
use lilo_session_core::{RpcResponse, Session, SessionState, SpawnRequest, SpawnResponse};
use lilo_session_driver::{SpawnLaunch, runtime_spawn_request};
use lilo_session_store::{PendingSpawnIntent, SessionDraft, SessionSpawnIntent};
use sqlx::SqliteConnection;
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
        let payload = match self
            .runtime
            .handle_rpc(
                context.principal.clone(),
                RuntimeRpc::Spawn {
                    request: runtime_request,
                },
            )
            .await
        {
            RuntimeResponse::Spawned(payload) => payload,
            response => {
                self.abort_spawn_intent(id, &runtime_spawn_failure(&response))
                    .await?;
                anyhow::bail!("runtime spawn failed: {}", runtime_spawn_failure(&response));
            }
        };
        let session = self
            .complete_spawn_intent(
                &intent,
                payload.lifecycle,
                payload.event,
                payload.stdout_path,
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
            .store
            .pool()
            .acquire()
            .await
            .context("failed to acquire session spawn Tx A connection")?;
        begin_immediate_tx(&mut conn, "BEGIN IMMEDIATE", "session spawn Tx A").await?;
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
            self.abort_spawn_intent(intent.session_id, "namespace deleted before session commit")
                .await?;
            let _ = self
                .runtime
                .handle_rpc(
                    Principal::Local(nix::unistd::getuid().as_raw()),
                    RuntimeRpc::Kill {
                        request: KillRequest {
                            session_id: intent.session_id,
                            signal: RuntimeSignal::Term,
                            grace_secs: 5,
                        },
                    },
                )
                .await;
            anyhow::bail!(
                "namespace deleted before session commit: {}",
                session.namespace
            );
        }

        let lifecycle_store = self.lifecycle_store();
        let mut conn = self
            .store
            .pool()
            .acquire()
            .await
            .context("failed to acquire session spawn Tx B connection")?;
        begin_immediate_tx(&mut conn, "BEGIN IMMEDIATE", "session spawn Tx B").await?;
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
        finish_immediate_tx(&mut conn, result, "session spawn Tx B").await?;

        self.runtime
            .append_event(event)
            .await
            .context("failed to append runtime event after session commit")?;
        Ok(session)
    }

    async fn abort_spawn_intent(&self, session_id: Uuid, reason: &str) -> Result<()> {
        let lifecycle_store = self.lifecycle_store();
        let mut conn = self
            .store
            .pool()
            .acquire()
            .await
            .context("failed to acquire session spawn abort tx connection")?;
        begin_immediate_tx(&mut conn, "BEGIN IMMEDIATE", "session spawn abort tx").await?;
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
            self.reconcile_pending_spawn_intent(intent).await?;
        }
        Ok(())
    }

    async fn reconcile_pending_spawn_intent(&self, intent: SessionSpawnIntent) -> Result<()> {
        let response = self
            .runtime
            .handle_rpc(
                lilo_im_core::Principal::Local(nix::unistd::getuid().as_raw()),
                RuntimeRpc::Status {
                    request: StatusRequest {
                        session_id: Some(intent.session_id),
                        session_ids: Vec::new(),
                        updated_since: None,
                        runtime: None,
                        state: None,
                    },
                },
            )
            .await;
        let RuntimeResponse::Status(status) = response else {
            self.abort_spawn_intent(intent.session_id, &runtime_spawn_failure(&response))
                .await?;
            return Ok(());
        };
        let Some(lifecycle) = status
            .lifecycles
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
        self.complete_spawn_intent(&pending, lifecycle, event, None)
            .await?;
        Ok(())
    }

    fn lifecycle_store(&self) -> LifecycleStore {
        LifecycleStore::from_pool(self.store.pool().clone())
    }
}

async fn begin_immediate_tx(
    conn: &mut SqliteConnection,
    begin_sql: &'static str,
    label: &str,
) -> Result<()> {
    sqlx::query(begin_sql)
        .execute(conn)
        .await
        .with_context(|| format!("failed to begin {label}"))
        .map(|_| ())
}

async fn finish_immediate_tx<T>(
    conn: &mut SqliteConnection,
    result: Result<T>,
    label: &str,
) -> Result<T> {
    match result {
        Ok(value) => {
            sqlx::query("COMMIT")
                .execute(conn)
                .await
                .with_context(|| format!("failed to commit {label}"))?;
            Ok(value)
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(conn).await;
            Err(error)
        }
    }
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

fn runtime_spawn_failure(response: &RuntimeResponse) -> String {
    match response {
        RuntimeResponse::SpawnConflict(payload) => {
            format!("spawn conflict: {:?}", payload.kind)
        }
        RuntimeResponse::Error(error) => error.message.clone(),
        other => format!("unexpected runtime response: {other:?}"),
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
