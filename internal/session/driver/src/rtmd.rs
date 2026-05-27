use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use lilo_rm_client::{ClientError, RuntimeClient};
use lilo_rm_core::{
    CaptureRequest, KillOutcome, KillRequest, Lifecycle, LifecycleState, NudgeFailureReason,
    NudgeOutcome, NudgeRequest, RuntimeKind as RtmdRuntimeKind, RuntimeSignal, SpawnConflictKind,
    SpawnConflictPayload, SpawnRequest, SpawnTarget as RtmdSpawnTarget, StatusFilter,
    ValidateTargetOutcome,
};
use lilo_session_core::RuntimeKind;
use tokio::time::{Instant, sleep};
use uuid::Uuid;

use crate::conv::{
    kill_outcome_label, lifecycle_state_label, lifecycle_to_probe, lifecycle_transcript_path,
};
use crate::driver::{
    CaptureResult, ChildExit, DriverError, DriverProbe, NudgeResult, SpawnDriver, SpawnLaunch,
    SpawnedProcess,
};

#[derive(Clone, Debug)]
pub struct RtmdDriver {
    client: RuntimeClient,
    terminal_sessions: Arc<Mutex<HashSet<Uuid>>>,
}

impl RtmdDriver {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            client: RuntimeClient::new(socket_path),
            terminal_sessions: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn client(&self) -> &RuntimeClient {
        &self.client
    }
}

#[async_trait]
impl SpawnDriver for RtmdDriver {
    async fn spawn(
        &self,
        session_id: &str,
        launch: &SpawnLaunch,
    ) -> Result<SpawnedProcess, DriverError> {
        let session_id = parse_session_id(session_id)?;
        self.locked_terminal_sessions().remove(&session_id);
        let payload = self
            .client
            .spawn(SpawnRequest {
                session_id,
                runtime: runtime_kind(launch.runtime),
                isolation: launch.isolation.clone(),
                image: launch.image.clone(),
                env: launch.env.clone(),
                mounts: launch.mounts.clone(),
                cwd: launch.cwd.clone(),
                target: runtime_target(&launch.target)?,
                force: launch.force,
                shell_resume: launch.shell_resume.clone(),
            })
            .await
            .map_err(spawn_error)?;
        let runtime_pid = runtime_pid(&payload.lifecycle)?;

        Ok(SpawnedProcess {
            runtime_pid,
            log_dir: payload.log_dir,
            stdout_path: payload.stdout_path,
            stderr_path: payload.stderr_path,
            tmux_pane: payload.lifecycle.tmux_pane.map(|pane| pane.to_string()),
        })
    }

    async fn validate_target(&self, target: &str) -> Result<(), DriverError> {
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

    async fn capture(
        &self,
        session_id: &str,
        scrollback_lines: Option<u32>,
    ) -> Result<CaptureResult, DriverError> {
        let session_id = parse_session_id(session_id)?;
        Ok(CaptureResult {
            response: self
                .client
                .capture(CaptureRequest {
                    session_id,
                    scrollback_lines,
                })
                .await?,
        })
    }

    async fn reap_exited(&self) -> Result<Vec<ChildExit>, DriverError> {
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

    async fn probe_session(
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

    async fn terminate(
        &self,
        session_id: &str,
        signal: &str,
        grace: Duration,
    ) -> Result<Option<ChildExit>, DriverError> {
        let session_id = parse_session_id(session_id)?;
        let signal = RuntimeSignal::from_str(signal)
            .map_err(|_| DriverError::InvalidSignal(signal.to_string()))?;
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
                self.wait_for_terminal(session_id, grace).await?
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

    async fn nudge(&self, session_id: &str, content: &str) -> Result<NudgeResult, DriverError> {
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

    fn terminate_all(&self) {}
}

impl RtmdDriver {
    fn locked_terminal_sessions(&self) -> MutexGuard<'_, HashSet<Uuid>> {
        self.terminal_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    async fn wait_for_terminal(
        &self,
        session_id: Uuid,
        grace: Duration,
    ) -> Result<Option<ChildExit>, DriverError> {
        let timeout = grace.max(Duration::from_secs(1));
        let deadline = Instant::now() + timeout;
        loop {
            let payload = self.client.status(status_session(session_id)).await?;
            let exit = payload
                .lifecycles
                .iter()
                .find(|lifecycle| lifecycle.session_id == session_id)
                .map(terminal_child_exit)
                .transpose()?
                .flatten();
            if exit.is_some() || Instant::now() >= deadline {
                return Ok(exit);
            }
            sleep(Duration::from_millis(100)).await;
        }
    }
}

fn parse_session_id(session_id: &str) -> Result<Uuid, DriverError> {
    Uuid::parse_str(session_id).map_err(|_| DriverError::InvalidSessionId(session_id.to_string()))
}

fn runtime_kind(runtime: RuntimeKind) -> RtmdRuntimeKind {
    match runtime {
        RuntimeKind::Claude => RtmdRuntimeKind::Claude,
        RuntimeKind::Codex => RtmdRuntimeKind::Codex,
    }
}

fn runtime_target(target: &str) -> Result<RtmdSpawnTarget, DriverError> {
    target
        .parse()
        .map_err(|error| DriverError::InvalidTarget(format!("{error}")))
}

fn runtime_pid(lifecycle: &Lifecycle) -> Result<u32, DriverError> {
    lifecycle
        .runtime_pid
        .ok_or_else(|| DriverError::MissingRuntimePid(lifecycle.session_id.to_string()))
}

fn nudge_result(outcome: &NudgeOutcome) -> NudgeResult {
    match outcome {
        NudgeOutcome::Delivered => NudgeResult {
            delivered: true,
            message: "delivered".to_string(),
        },
        NudgeOutcome::Unsupported(NudgeFailureReason::HeadlessLifecycle) => NudgeResult {
            delivered: false,
            message: "headless runtime does not support nudges".to_string(),
        },
        NudgeOutcome::Failed(NudgeFailureReason::SessionEnded) => NudgeResult {
            delivered: false,
            message: "session ended before the nudge could land".to_string(),
        },
        NudgeOutcome::Failed(NudgeFailureReason::TmuxPaneDead) => NudgeResult {
            delivered: false,
            message: "tmux pane is no longer available".to_string(),
        },
        NudgeOutcome::Unsupported(reason) => NudgeResult {
            delivered: false,
            message: format!("nudge unsupported ({})", reason.as_str()),
        },
        NudgeOutcome::Failed(reason) => NudgeResult {
            delivered: false,
            message: format!("nudge failed ({})", reason.as_str()),
        },
    }
}

fn terminal_child_exit(lifecycle: &Lifecycle) -> Result<Option<ChildExit>, DriverError> {
    let exit_code = match lifecycle.state {
        LifecycleState::Forking | LifecycleState::Running => return Ok(None),
        LifecycleState::Exited(exit) => exit.code,
        LifecycleState::Lost(_) => None,
        _ => {
            return Err(DriverError::UnknownRuntimeVariant {
                variant: lifecycle_state_label(&lifecycle.state),
            });
        }
    };
    Ok(Some(ChildExit {
        session_id: lifecycle.session_id.to_string(),
        runtime_pid: lifecycle.runtime_pid.unwrap_or_default(),
        exit_code,
        transcript_path: lifecycle_transcript_path(lifecycle),
    }))
}

fn status_session(session_id: Uuid) -> StatusFilter {
    StatusFilter {
        session_id: Some(session_id),
        session_ids: Vec::new(),
        updated_since: None,
        runtime: None,
        state: None,
    }
}

fn spawn_error(error: ClientError) -> DriverError {
    match error {
        ClientError::SpawnConflict(payload) => spawn_conflict(payload.as_ref()),
        other => DriverError::Client(other),
    }
}

fn spawn_conflict(payload: &SpawnConflictPayload) -> DriverError {
    DriverError::SpawnConflict {
        kind: payload.kind,
        message: format_spawn_conflict(payload),
    }
}

fn format_spawn_conflict(payload: &SpawnConflictPayload) -> String {
    let lifecycle = &payload.lifecycle;
    let runtime: &str = match &lifecycle.runtime {
        RtmdRuntimeKind::Claude => "claude",
        RtmdRuntimeKind::Codex => "codex",
        RtmdRuntimeKind::Other(name) => name.as_str(),
    };
    let session_id = lifecycle.session_id;
    let pid = lifecycle
        .runtime_pid
        .map(|pid| format!(" (pid {pid})"))
        .unwrap_or_default();
    match payload.kind {
        SpawnConflictKind::TmuxPaneOccupancy => {
            let pane = lifecycle
                .tmux_pane
                .as_ref()
                .map_or_else(|| "<unknown>".to_string(), ToString::to_string);
            format!("tmux pane {pane} is already running {runtime} session {session_id}{pid}")
        }
        SpawnConflictKind::SessionId => {
            format!("session {session_id} is already running {runtime}{pid}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::OrPanic as _;
    use lilo_rm_core::{IsolationPolicy, TmuxAddress};

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
        let message = format_spawn_conflict(&payload);
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
        let message = format_spawn_conflict(&payload);
        assert_eq!(
            message,
            "session 00000000-0000-0000-0000-000000000000 is already running claude (pid 29032)"
        );
    }
}
