use std::{collections::HashMap, sync::Arc};

use anyhow::{Result, bail};
use lilo_rm_core::{
    LaunchSpec, Lifecycle, LifecycleState, RuntimeEvent, ShimReady, SpawnRequest,
    ValidateTargetOutcome, ValidateTargetRequest, ValidateTargetResponse,
};
use tokio::sync::{Mutex, oneshot};
use uuid::Uuid;

use crate::{error::RuntimeFailure, event_channel};

use super::ServerState;

pub(super) struct SpawnCoordinator {
    pending_launches: Mutex<HashMap<Uuid, LaunchSpec>>,
    pending_ready: Mutex<HashMap<Uuid, oneshot::Sender<ShimReady>>>,
}

pub(crate) struct BeginSpawn {
    pub(crate) ready: oneshot::Receiver<ShimReady>,
    pub(crate) session_backed: bool,
}

impl SpawnCoordinator {
    pub(super) fn new() -> Self {
        Self {
            pending_launches: Mutex::new(HashMap::new()),
            pending_ready: Mutex::new(HashMap::new()),
        }
    }

    pub(super) async fn begin_spawn(
        &self,
        state: &ServerState,
        request: &SpawnRequest,
        launch: LaunchSpec,
    ) -> Result<BeginSpawn> {
        let session_backed = match state.store().get(request.session_id).await? {
            Some(lifecycle)
                if lifecycle.state == LifecycleState::Forking
                    && lifecycle.runtime == request.runtime
                    && lifecycle.isolation == request.isolation =>
            {
                true
            }
            Some(_) => return Err(RuntimeFailure::session_already_exists(request.session_id)),
            None => false,
        };
        self.validate_spawn_target(state, request).await?;

        if !session_backed {
            let mut lifecycle = Lifecycle::forking(request.session_id, request.runtime.clone());
            lifecycle.isolation = request.isolation.clone();
            state.store().insert_forking(&lifecycle).await?;
        }
        self.pending_launches
            .lock()
            .await
            .insert(request.session_id, launch);
        match self.begin_ready_wait(request.session_id).await {
            Ok(ready) => Ok(BeginSpawn {
                ready,
                session_backed,
            }),
            Err(error) => {
                self.cancel_spawn(state, request.session_id).await;
                Err(error)
            }
        }
    }

    async fn validate_spawn_target(
        &self,
        state: &ServerState,
        request: &SpawnRequest,
    ) -> Result<()> {
        match self.validate_target(state, &request.target).await?.outcome {
            ValidateTargetOutcome::Valid => Ok(()),
            ValidateTargetOutcome::TmuxPaneDead { address } => {
                Err(RuntimeFailure::tmux_pane_dead(address))
            }
            ValidateTargetOutcome::InvalidTarget { message } => {
                Err(RuntimeFailure::protocol_mismatch(message))
            }
            ValidateTargetOutcome::UnsupportedTarget { target } => Err(
                RuntimeFailure::protocol_mismatch(format!("unsupported target {target}")),
            ),
        }
    }

    pub(super) async fn validate_target_request(
        &self,
        state: &ServerState,
        request: ValidateTargetRequest,
    ) -> Result<ValidateTargetResponse> {
        let target = match request.target.parse() {
            Ok(target) => target,
            Err(error) => return Ok(ValidateTargetResponse::from_target_parse_error(error)),
        };
        self.validate_target(state, &target).await
    }

    async fn validate_target(
        &self,
        state: &ServerState,
        target: &lilo_rm_core::SpawnTarget,
    ) -> Result<ValidateTargetResponse> {
        if let Some(address) = target.tmux_address()
            && !lilo_runtime_platform::tmux::TmuxGateway::is_alive(
                state.config().tmux_server_label.as_deref(),
                address,
            )
            .await?
        {
            return Ok(ValidateTargetResponse::tmux_pane_dead(address.clone()));
        }
        Ok(ValidateTargetResponse::valid())
    }

    async fn begin_ready_wait(&self, session_id: Uuid) -> Result<oneshot::Receiver<ShimReady>> {
        let (sender, receiver) = oneshot::channel();
        let previous = self.pending_ready.lock().await.insert(session_id, sender);
        if previous.is_some() {
            return Err(RuntimeFailure::protocol_mismatch(format!(
                "session {session_id} already has a pending shim"
            )));
        }
        Ok(receiver)
    }

    pub(super) async fn cancel_spawn(&self, state: &ServerState, session_id: Uuid) {
        self.pending_launches.lock().await.remove(&session_id);
        self.pending_ready.lock().await.remove(&session_id);
        if let Err(error) = state.store().delete(session_id).await {
            tracing::warn!(%error, %session_id, "failed to delete canceled lifecycle");
        }
    }

    pub(super) async fn take_launch_spec(&self, session_id: Uuid) -> Result<LaunchSpec> {
        self.pending_launches
            .lock()
            .await
            .remove(&session_id)
            .ok_or_else(|| {
                RuntimeFailure::protocol_mismatch(format!(
                    "no pending launch for session {session_id}"
                ))
            })
    }

    pub(super) async fn complete_shim_ready(
        &self,
        state: &Arc<ServerState>,
        ready: ShimReady,
    ) -> Result<()> {
        let sender = self.pending_ready.lock().await.remove(&ready.session_id);
        if let Some(sender) = sender {
            return sender.send(ready).map_err(|ready| {
                RuntimeFailure::protocol_mismatch(format!(
                    "spawn waiter dropped for session {}",
                    ready.session_id
                ))
            });
        }
        self.record_reconnected_ready(state, ready)
            .await
            .map(|_| ())
    }

    pub(super) async fn record_running(
        &self,
        state: &Arc<ServerState>,
        request: &SpawnRequest,
        ready: ShimReady,
        append_event: bool,
    ) -> Result<(Lifecycle, RuntimeEvent)> {
        let runtime_pid = ready.runtime_pid;
        let mut lifecycle = state
            .store()
            .get(request.session_id)
            .await?
            .ok_or_else(|| RuntimeFailure::session_not_found(request.session_id))?;
        if lifecycle.runtime != request.runtime {
            return Err(RuntimeFailure::protocol_mismatch(format!(
                "runtime mismatch for session {}",
                request.session_id
            )));
        }
        if !lifecycle.mark_running(ready) {
            return Err(RuntimeFailure::protocol_mismatch(format!(
                "session {} is not waiting for ShimReady",
                request.session_id
            )));
        }
        lifecycle.tmux_pane = request.target.tmux_address().cloned();
        state.populate_log_availability(&mut lifecycle).await;
        state.store().update_lifecycle(&lifecycle).await?;
        let event = event_channel::running_event(&lifecycle)?;

        state
            .start_exit_watcher(request.session_id, runtime_pid)
            .await?;
        let event = if append_event {
            state.append_event(event).await?
        } else {
            event
        };
        Ok((lifecycle, event))
    }

    async fn record_reconnected_ready(
        &self,
        state: &Arc<ServerState>,
        ready: ShimReady,
    ) -> Result<Option<RuntimeEvent>> {
        let runtime_pid = ready.runtime_pid;
        let mut lifecycle = state
            .store()
            .get(ready.session_id)
            .await?
            .ok_or_else(|| RuntimeFailure::session_not_found(ready.session_id))?;
        match lifecycle.state {
            LifecycleState::Forking => {
                lifecycle.mark_running(ready);
                state.store().update_lifecycle(&lifecycle).await?;
                let event = event_channel::running_event(&lifecycle)?;
                state
                    .start_exit_watcher(lifecycle.session_id, runtime_pid)
                    .await?;
                Ok(Some(state.append_event(event).await?))
            }
            LifecycleState::Running => {
                state
                    .start_exit_watcher(lifecycle.session_id, runtime_pid)
                    .await?;
                Ok(None)
            }
            LifecycleState::Exited(_) | LifecycleState::Lost(_) => {
                bail!("session {} is already terminal", lifecycle.session_id)
            }
            _ => bail!(
                "reconnecting ShimReady for session {} saw unsupported lifecycle state variant: {:?}",
                lifecycle.session_id,
                lifecycle.state
            ),
        }
    }

    pub(super) async fn pending_shim_socket_count(&self) -> usize {
        self.pending_ready.lock().await.len()
    }
}
