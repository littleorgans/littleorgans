use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use lilo_common::id::SessionId;
use lilo_rm_core::{
    CaptureRequest, EventBatch, EventsRequest, KillOutcome, KillRequest, Lifecycle, NudgeMode,
    NudgeRequest, RuntimeSignal, SpawnRequest as RuntimeSpawnRequest, StatusFilter,
};
use lilo_runtime_daemon::RuntimeService;
use lilo_session_core::RuntimeDoctorReport;

use crate::conv::{
    capture_result, kill_outcome_label, nudge_result, runtime_doctor_error, runtime_doctor_report,
    spawn_outcome, terminal_child_exit,
};
use crate::driver::{CaptureResult, ChildExit, NudgeResult, RuntimeError, SpawnedProcess};
use crate::port::{RuntimePort, RuntimePortFuture, wait_for_terminal};

#[derive(Clone)]
pub struct InProcessRuntime {
    runtime: Arc<RuntimeService>,
    terminal_sessions: Arc<Mutex<HashSet<SessionId>>>,
}

impl InProcessRuntime {
    pub fn new(runtime: Arc<RuntimeService>) -> Self {
        Self {
            runtime,
            terminal_sessions: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn locked_terminal_sessions(&self) -> MutexGuard<'_, HashSet<SessionId>> {
        self.terminal_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn domain_error(error: impl std::fmt::Display) -> RuntimeError {
        RuntimeError::local(error)
    }
}

impl RuntimePort for InProcessRuntime {
    fn spawn(&self, request: RuntimeSpawnRequest) -> RuntimePortFuture<'_, SpawnedProcess> {
        Box::pin(async move {
            self.locked_terminal_sessions().remove(&request.session_id);
            let outcome = self
                .runtime
                .spawn(request)
                .await
                .map_err(Self::domain_error)?;
            spawn_outcome(outcome)
        })
    }

    fn reap_exited(&self) -> RuntimePortFuture<'_, Vec<ChildExit>> {
        Box::pin(async move {
            let lifecycles = self.runtime.status(StatusFilter::empty()).await;
            let mut terminal_sessions = self.locked_terminal_sessions();
            let mut exits = Vec::new();
            for lifecycle in lifecycles {
                if let Some(exit) = terminal_child_exit(&lifecycle)?
                    && terminal_sessions.insert(lifecycle.session_id)
                {
                    exits.push(exit);
                }
            }
            Ok(exits)
        })
    }

    fn capture(
        &self,
        session_id: SessionId,
        scrollback_lines: Option<u32>,
    ) -> RuntimePortFuture<'_, CaptureResult> {
        Box::pin(async move {
            let response = self
                .runtime
                .capture(CaptureRequest {
                    session_id,
                    scrollback_lines,
                })
                .await
                .map_err(Self::domain_error)?;
            Ok(capture_result(response))
        })
    }

    fn terminate(
        &self,
        session_id: SessionId,
        signal: RuntimeSignal,
        grace: Duration,
    ) -> RuntimePortFuture<'_, Option<ChildExit>> {
        Box::pin(async move {
            let outcome = self
                .runtime
                .kill_runtime(KillRequest {
                    session_id,
                    signal,
                    grace_secs: grace.as_secs(),
                })
                .await
                .map_err(Self::domain_error)?;

            let exit = match outcome {
                KillOutcome::Signalled | KillOutcome::AlreadyExited => {
                    wait_for_terminal(self, session_id, grace).await?
                }
                _ => {
                    return Err(RuntimeError::local(format!(
                        "unknown runtime variant: {}",
                        kill_outcome_label(outcome)
                    )));
                }
            };
            if exit.is_some() {
                self.locked_terminal_sessions().insert(session_id);
            }
            Ok(exit)
        })
    }

    fn nudge<'a>(
        &'a self,
        session_id: SessionId,
        content: &'a str,
        mode: NudgeMode,
        timeout_ms: Option<u64>,
    ) -> RuntimePortFuture<'a, NudgeResult> {
        Box::pin(async move {
            let response = self
                .runtime
                .nudge_runtime(NudgeRequest {
                    session_id,
                    content: content.to_string(),
                    mode,
                    timeout_ms,
                })
                .await
                .map_err(Self::domain_error)?;
            Ok(nudge_result(&response.outcome))
        })
    }

    fn status(&self, filter: StatusFilter) -> RuntimePortFuture<'_, Vec<Lifecycle>> {
        Box::pin(async move { Ok(self.runtime.status(filter).await) })
    }

    fn poll_events(&self, request: EventsRequest) -> RuntimePortFuture<'_, EventBatch> {
        Box::pin(async move { Ok(self.runtime.poll_events(request).await) })
    }

    fn doctor(&self) -> RuntimePortFuture<'_, RuntimeDoctorReport> {
        Box::pin(async move {
            Ok(match self.runtime.doctor().await {
                Ok(doctor) => runtime_doctor_report(doctor, None),
                Err(error) => runtime_doctor_error(None, error.to_string(), None),
            })
        })
    }

    fn terminate_all(&self) {
        self.runtime.drain_shims();
    }
}
