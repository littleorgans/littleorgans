use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use lilo_rm_core::{
    KillByPidRequest, KillByPidResponse, KillOutcome, Lifecycle, LifecycleState, LostEvidence,
    RuntimeEvent, RuntimeExit, RuntimeSignal, ShimExit, TerminationEvidence,
};
use lilo_runtime_store::LifecycleStore;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{error::RuntimeFailure, event_channel};

use super::ServerState;

pub(super) struct TerminationCoordinator {
    terminated_events: Mutex<HashSet<Uuid>>,
}

impl TerminationCoordinator {
    pub(super) fn new() -> Self {
        Self {
            terminated_events: Mutex::new(HashSet::new()),
        }
    }

    pub(super) async fn kill_pid(&self, request: KillByPidRequest) -> Result<KillByPidResponse> {
        let outcome =
            lilo_runtime_platform::signal::send_raw_signal_for_kill(request.pid, request.signal)?;
        if matches!(outcome, KillOutcome::AlreadyExited) {
            return Ok(KillByPidResponse {
                pid: request.pid,
                signal: request.signal,
                killed_after_grace: false,
                outcome,
            });
        }
        let deadline = Instant::now() + Duration::from_secs(request.grace_secs);

        while Instant::now() < deadline {
            if !lilo_runtime_platform::process::pid_alive(request.pid) {
                return Ok(KillByPidResponse {
                    pid: request.pid,
                    signal: request.signal,
                    killed_after_grace: false,
                    outcome,
                });
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        let mut killed_after_grace = false;
        let kill_signal = lilo_runtime_platform::signal::signal_number(RuntimeSignal::Kill);
        if lilo_runtime_platform::process::pid_alive(request.pid) && request.signal != kill_signal {
            lilo_runtime_platform::signal::send_raw_signal(request.pid, kill_signal)?;
            killed_after_grace = true;
        }
        Ok(KillByPidResponse {
            pid: request.pid,
            signal: request.signal,
            killed_after_grace,
            outcome,
        })
    }

    pub(super) async fn record_shim_exit(
        &self,
        state: &ServerState,
        exit: ShimExit,
    ) -> Result<Option<RuntimeEvent>> {
        self.record_exited(
            state,
            exit.session_id,
            exit.exit,
            TerminationEvidence::ShimExit,
        )
        .await
    }

    pub(super) async fn is_terminal(&self, store: &LifecycleStore, session_id: Uuid) -> bool {
        store
            .get(session_id)
            .await
            .ok()
            .flatten()
            .is_some_and(|lifecycle| {
                matches!(
                    lifecycle.state,
                    LifecycleState::Exited(_) | LifecycleState::Lost(_)
                )
            })
    }

    pub(super) async fn watcher_evidence(
        &self,
        store: &LifecycleStore,
        session_id: Uuid,
    ) -> Result<TerminationEvidence> {
        let shim_pid = store
            .get(session_id)
            .await?
            .and_then(|lifecycle| lifecycle.shim_pid)
            .ok_or_else(|| anyhow!("session {session_id} missing shim pid"))?;
        if lilo_runtime_platform::process::pid_alive(shim_pid) {
            Ok(TerminationEvidence::ProcessExit)
        } else {
            Ok(TerminationEvidence::Lost(
                LostEvidence::ShimDiedBeforeReport,
            ))
        }
    }

    pub(super) async fn record_exited(
        &self,
        state: &ServerState,
        session_id: Uuid,
        exit: RuntimeExit,
        evidence: TerminationEvidence,
    ) -> Result<Option<RuntimeEvent>> {
        self.record_terminal(state, session_id, evidence, |lifecycle| {
            lifecycle.mark_exited(exit)
        })
        .await
    }

    async fn record_terminal<F>(
        &self,
        state: &ServerState,
        session_id: Uuid,
        evidence: TerminationEvidence,
        mark_terminal: F,
    ) -> Result<Option<RuntimeEvent>>
    where
        F: FnOnce(&mut Lifecycle) -> bool + Send,
    {
        let mut lifecycle = state
            .store()
            .get(session_id)
            .await?
            .ok_or_else(|| RuntimeFailure::session_not_found(session_id))?;
        if !mark_terminal(&mut lifecycle) {
            return Ok(None);
        }
        state.store().update_lifecycle(&lifecycle).await?;
        self.finish_terminal(state, session_id, &lifecycle, evidence)
            .await
    }

    pub(super) async fn record_lost(
        &self,
        state: &ServerState,
        session_id: Uuid,
        evidence: LostEvidence,
    ) -> Result<Option<RuntimeEvent>> {
        self.record_terminal(
            state,
            session_id,
            TerminationEvidence::Lost(evidence),
            |lifecycle| lifecycle.mark_lost(evidence),
        )
        .await
    }

    async fn finish_terminal(
        &self,
        state: &ServerState,
        session_id: Uuid,
        lifecycle: &Lifecycle,
        evidence: TerminationEvidence,
    ) -> Result<Option<RuntimeEvent>> {
        state.remove_watcher(session_id).await;
        if !self.terminated_events.lock().await.insert(session_id) {
            return Ok(None);
        }
        let event = match evidence {
            TerminationEvidence::Lost(lost) => event_channel::lost_event(lifecycle, lost),
            TerminationEvidence::ShimExit | TerminationEvidence::ProcessExit => {
                event_channel::terminated_event(lifecycle, evidence)
            }
            _ => bail!(
                "recording terminal event for session {session_id} received unsupported termination evidence variant: {evidence:?}"
            ),
        };
        Ok(Some(state.append_event(event).await?))
    }
}
