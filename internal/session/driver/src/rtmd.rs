use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use lilo_rm_client::{ClientError, RuntimeClient};
use lilo_rm_core::{
    CaptureRequest, EventBatch, EventsRequest, KillOutcome, KillRequest, Lifecycle, NudgeRequest,
    StatusFilter, ValidateTargetOutcome,
};
use lilo_session_core::RuntimeDoctorReport;
use uuid::Uuid;

use crate::conv::{
    capture_result, kill_outcome_label, lifecycle_to_probe, nudge_result, parse_runtime_signal,
    parse_session_id, runtime_doctor_error, runtime_doctor_report, runtime_spawn_request,
    spawned_process, status_session, terminal_child_exit,
};
use crate::driver::{
    CaptureResult, ChildExit, DriverError, DriverProbe, NudgeResult, SpawnLaunch, SpawnedProcess,
};
use crate::port::{RuntimePort, RuntimePortFuture, wait_for_terminal};

#[derive(Clone, Debug)]
pub struct RtmdDriver {
    client: RuntimeClient,
    socket_path: PathBuf,
    terminal_sessions: Arc<Mutex<HashSet<Uuid>>>,
}

impl RtmdDriver {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            client: RuntimeClient::new(socket_path.clone()),
            socket_path,
            terminal_sessions: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn client(&self) -> &RuntimeClient {
        &self.client
    }
}

impl RtmdDriver {
    pub async fn spawn(
        &self,
        session_id: &str,
        launch: &SpawnLaunch,
    ) -> Result<SpawnedProcess, DriverError> {
        let session_id = parse_session_id(session_id)?;
        self.locked_terminal_sessions().remove(&session_id);
        let request = runtime_spawn_request(session_id, launch)?;
        let payload = self.client.spawn(request).await.map_err(spawn_error)?;
        spawned_process(payload)
    }

    pub async fn validate_target(&self, target: &str) -> Result<(), DriverError> {
        match self.client.validate_target(target).await?.outcome {
            ValidateTargetOutcome::Valid => Ok(()),
            ValidateTargetOutcome::InvalidTarget { message } => {
                Err(DriverError::InvalidTarget(message))
            }
            ValidateTargetOutcome::TmuxPaneDead { address } => {
                Err(DriverError::TmuxPaneDead(address.to_string()))
            }
            ValidateTargetOutcome::UnsupportedTarget { target } => {
                Err(DriverError::UnsupportedTarget(target))
            }
        }
    }

    pub async fn capture(
        &self,
        session_id: &str,
        scrollback_lines: Option<u32>,
    ) -> Result<CaptureResult, DriverError> {
        let session_id = parse_session_id(session_id)?;
        let response = self
            .client
            .capture(CaptureRequest {
                session_id,
                scrollback_lines,
            })
            .await?;
        Ok(capture_result(response))
    }

    pub async fn reap_exited(&self) -> Result<Vec<ChildExit>, DriverError> {
        let payload = self.client.status(StatusFilter::empty()).await?;
        let mut terminal_sessions = self.locked_terminal_sessions();
        let mut exits = Vec::new();
        for lifecycle in payload.lifecycles {
            if let Some(exit) = terminal_child_exit(&lifecycle)?
                && terminal_sessions.insert(lifecycle.session_id)
            {
                exits.push(exit);
            }
        }
        Ok(exits)
    }

    pub async fn probe_session(
        &self,
        session_id: &str,
        runtime_pid: u32,
    ) -> Result<DriverProbe, DriverError> {
        let session_id = parse_session_id(session_id)?;
        let payload = self.client.status(status_session(session_id)).await?;
        let Some(lifecycle) = payload
            .lifecycles
            .iter()
            .find(|lifecycle| lifecycle.session_id == session_id)
        else {
            return Ok(DriverProbe {
                verified: false,
                evidence: format!("rtmd has no lifecycle for session {session_id}"),
                transcript_path: None,
            });
        };
        lifecycle_to_probe(lifecycle, runtime_pid)
    }

    pub async fn terminate(
        &self,
        session_id: &str,
        signal: &str,
        grace: Duration,
    ) -> Result<Option<ChildExit>, DriverError> {
        let session_id = parse_session_id(session_id)?;
        let signal = parse_runtime_signal(signal)?;
        let outcome = self
            .client
            .kill(KillRequest {
                session_id,
                signal,
                grace_secs: grace.as_secs(),
            })
            .await?;

        let exit = match outcome {
            KillOutcome::Signalled | KillOutcome::AlreadyExited => {
                wait_for_terminal(self, session_id, grace).await?
            }
            _ => {
                return Err(DriverError::UnknownRuntimeVariant {
                    variant: kill_outcome_label(outcome),
                });
            }
        };
        if exit.is_some() {
            self.locked_terminal_sessions().insert(session_id);
        }
        Ok(exit)
    }

    pub async fn nudge(&self, session_id: &str, content: &str) -> Result<NudgeResult, DriverError> {
        let session_id = parse_session_id(session_id)?;
        let response = self
            .client
            .nudge(NudgeRequest {
                session_id,
                content: content.to_string(),
            })
            .await?;
        Ok(nudge_result(&response.outcome))
    }

    pub async fn status(&self, filter: StatusFilter) -> Result<Vec<Lifecycle>, DriverError> {
        Ok(self.client.status(filter).await?.lifecycles)
    }

    pub async fn poll_events(&self, request: EventsRequest) -> Result<EventBatch, DriverError> {
        Ok(self.client.events(request).await?)
    }

    pub async fn doctor(&self) -> Result<RuntimeDoctorReport, DriverError> {
        let socket_path = Some(self.socket_path.display().to_string());
        Ok(match self.client.doctor().await {
            Ok(payload) => runtime_doctor_report(payload.doctor, socket_path),
            Err(error) => runtime_doctor_error(
                Some(error.code().as_str().to_string()),
                error.to_string(),
                socket_path,
            ),
        })
    }

    pub fn terminate_all(&self) {}
}

impl RtmdDriver {
    fn locked_terminal_sessions(&self) -> MutexGuard<'_, HashSet<Uuid>> {
        self.terminal_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn spawn_error(error: ClientError) -> DriverError {
    match error {
        ClientError::SpawnConflict(payload) => crate::conv::spawn_conflict(payload.as_ref()),
        other => DriverError::Client(other),
    }
}

impl RuntimePort for RtmdDriver {
    fn spawn<'a>(
        &'a self,
        session_id: &'a str,
        launch: &'a SpawnLaunch,
    ) -> RuntimePortFuture<'a, SpawnedProcess> {
        Box::pin(async move { RtmdDriver::spawn(self, session_id, launch).await })
    }

    fn reap_exited(&self) -> RuntimePortFuture<'_, Vec<ChildExit>> {
        Box::pin(async move { RtmdDriver::reap_exited(self).await })
    }

    fn capture<'a>(
        &'a self,
        session_id: &'a str,
        scrollback_lines: Option<u32>,
    ) -> RuntimePortFuture<'a, CaptureResult> {
        Box::pin(async move { RtmdDriver::capture(self, session_id, scrollback_lines).await })
    }

    fn terminate<'a>(
        &'a self,
        session_id: &'a str,
        signal: &'a str,
        grace: Duration,
    ) -> RuntimePortFuture<'a, Option<ChildExit>> {
        Box::pin(async move { RtmdDriver::terminate(self, session_id, signal, grace).await })
    }

    fn nudge<'a>(
        &'a self,
        session_id: &'a str,
        content: &'a str,
    ) -> RuntimePortFuture<'a, NudgeResult> {
        Box::pin(async move { RtmdDriver::nudge(self, session_id, content).await })
    }

    fn status(&self, filter: StatusFilter) -> RuntimePortFuture<'_, Vec<Lifecycle>> {
        Box::pin(async move { RtmdDriver::status(self, filter).await })
    }

    fn poll_events(&self, request: EventsRequest) -> RuntimePortFuture<'_, EventBatch> {
        Box::pin(async move { RtmdDriver::poll_events(self, request).await })
    }

    fn doctor(&self) -> RuntimePortFuture<'_, RuntimeDoctorReport> {
        Box::pin(async move { RtmdDriver::doctor(self).await })
    }

    fn terminate_all(&self) {
        RtmdDriver::terminate_all(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::OrPanic as _;
    use lilo_rm_core::{
        IsolationPolicy, Lifecycle, LifecycleState, RuntimeKind as RtmdRuntimeKind,
        SpawnConflictKind, SpawnConflictPayload, TmuxAddress,
    };

    fn lifecycle(tmux_pane: Option<TmuxAddress>) -> Lifecycle {
        Lifecycle {
            session_id: Uuid::nil(),
            runtime: RtmdRuntimeKind::Claude,
            isolation: IsolationPolicy::default(),
            state: LifecycleState::Running,
            shim_pid: None,
            runtime_pid: Some(29032),
            start_time: None,
            tmux_pane,
            log_availability: None,
        }
    }

    #[test]
    fn tmux_pane_conflict_renders_human_message() {
        let payload = SpawnConflictPayload {
            kind: SpawnConflictKind::TmuxPaneOccupancy,
            lifecycle: lifecycle(Some("1:3.1".parse().or_panic("pane parses"))),
        };
        let message = crate::conv::spawn_conflict(&payload).to_string();
        assert_eq!(
            message,
            "tmux pane 1:3.1 is already running claude session 00000000-0000-0000-0000-000000000000 (pid 29032)"
        );
        assert!(!message.contains("Lifecycle {"));
    }

    #[test]
    fn session_id_conflict_renders_human_message() {
        let payload = SpawnConflictPayload {
            kind: SpawnConflictKind::SessionId,
            lifecycle: lifecycle(None),
        };
        let message = crate::conv::spawn_conflict(&payload).to_string();
        assert_eq!(
            message,
            "session 00000000-0000-0000-0000-000000000000 is already running claude (pid 29032)"
        );
    }
}
