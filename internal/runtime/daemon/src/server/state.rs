use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use lilo_identity_service::IdentityClient;
#[cfg(test)]
use lilo_im_store::SqliteAuditSink;
use lilo_rm_core::{
    CaptureError, CaptureRequest, CaptureResponse, EventsRequest, KillByPidRequest,
    KillByPidResponse, KillOutcome, KillRequest, LaunchSpec, Lifecycle, LifecycleLogAvailability,
    LostEvidence, NudgeFailureReason, NudgeOutcome, NudgeRequest, NudgeResponse, RuntimeEvent,
    RuntimeExit, ShimExit, ShimReady, SpawnRequest, StatusFilter, TerminationEvidence,
    ValidateTargetRequest, ValidateTargetResponse, WatcherCounts,
};
use lilo_runtime_store::LifecycleStore;
use uuid::Uuid;

use crate::{
    error::RuntimeFailure,
    event_log::{CursorExpired, EventLog, EventLogPage},
    runtime_kill,
};

use super::{
    DaemonConfig,
    events::EventAppender,
    spawn::{BeginSpawn, SpawnCoordinator},
    status::StatusReader,
    termination::TerminationCoordinator,
    watcher::WatcherCoordinator,
};

pub(crate) struct ServerState {
    config: DaemonConfig,
    store: LifecycleStore,
    identity: IdentityClient,
    started_instant: Instant,
    spawn: SpawnCoordinator,
    termination: TerminationCoordinator,
    watchers: WatcherCoordinator,
    status: StatusReader,
    events: EventAppender,
}

impl ServerState {
    #[cfg(test)]
    pub(crate) fn new(config: DaemonConfig, store: LifecycleStore) -> Result<Self> {
        let identity = IdentityClient::new(
            SqliteAuditSink::with_pool(store.pool().clone()),
            nix::unistd::getuid().as_raw(),
        );
        Self::new_with_identity(config, store, identity)
    }

    pub(crate) fn new_with_identity(
        config: DaemonConfig,
        store: LifecycleStore,
        identity: IdentityClient,
    ) -> Result<Self> {
        let event_log = EventLog::open(config.data_dir())?;
        Ok(Self {
            config,
            store,
            identity,
            started_instant: Instant::now(),
            spawn: SpawnCoordinator::new(),
            termination: TerminationCoordinator::new(),
            watchers: WatcherCoordinator::new(),
            status: StatusReader::new(),
            events: EventAppender::new(event_log),
        })
    }

    pub(crate) fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub(crate) fn store(&self) -> &LifecycleStore {
        &self.store
    }

    pub(crate) fn identity(&self) -> &IdentityClient {
        &self.identity
    }

    pub(crate) fn uptime_secs(&self) -> u64 {
        self.started_instant.elapsed().as_secs()
    }

    pub(crate) async fn begin_spawn(
        &self,
        request: &SpawnRequest,
        launch: LaunchSpec,
    ) -> Result<BeginSpawn> {
        self.spawn.begin_spawn(self, request, launch).await
    }

    pub(crate) async fn validate_target_request(
        &self,
        request: ValidateTargetRequest,
    ) -> Result<ValidateTargetResponse> {
        self.spawn.validate_target_request(request).await
    }

    pub(crate) async fn cancel_spawn(&self, session_id: Uuid) {
        self.spawn.cancel_spawn(self, session_id).await;
    }

    pub(crate) async fn take_launch_spec(&self, session_id: Uuid) -> Result<LaunchSpec> {
        self.spawn.take_launch_spec(session_id).await
    }

    pub(crate) async fn complete_shim_ready(self: &Arc<Self>, ready: ShimReady) -> Result<()> {
        self.spawn.complete_shim_ready(self, ready).await
    }

    pub(crate) async fn record_running(
        self: &Arc<Self>,
        request: &SpawnRequest,
        ready: ShimReady,
        append_event: bool,
    ) -> Result<(Lifecycle, RuntimeEvent)> {
        self.spawn
            .record_running(self, request, ready, append_event)
            .await
    }

    pub(crate) async fn kill_runtime(&self, request: KillRequest) -> Result<KillOutcome> {
        runtime_kill::kill_runtime(self, request).await
    }

    pub(crate) async fn kill_pid(&self, request: KillByPidRequest) -> Result<KillByPidResponse> {
        self.termination.kill_pid(request).await
    }

    pub(crate) async fn nudge_runtime(&self, request: NudgeRequest) -> Result<NudgeResponse> {
        let lifecycle = self
            .store
            .get(request.session_id)
            .await?
            .ok_or_else(|| RuntimeFailure::session_not_found(request.session_id))?;
        let Some(tmux_pane) = lifecycle.tmux_pane.as_ref() else {
            return Ok(NudgeResponse {
                delivered: false,
                outcome: NudgeOutcome::Unsupported(NudgeFailureReason::HeadlessLifecycle),
            });
        };
        if self.is_terminal(request.session_id).await {
            return Ok(NudgeResponse {
                delivered: false,
                outcome: NudgeOutcome::Failed(NudgeFailureReason::SessionEnded),
            });
        }

        if !lilo_runtime_platform::tmux::TmuxGateway::nudge(tmux_pane, &request.content).await? {
            return Ok(NudgeResponse {
                delivered: false,
                outcome: NudgeOutcome::Failed(NudgeFailureReason::TmuxPaneDead),
            });
        }

        Ok(NudgeResponse {
            delivered: true,
            outcome: NudgeOutcome::Delivered,
        })
    }

    pub(crate) async fn capture_pane(&self, request: CaptureRequest) -> Result<CaptureResponse> {
        let Some(lifecycle) = self.store.get(request.session_id).await? else {
            return Ok(CaptureResponse::Failed(CaptureError::SessionMissing));
        };
        let Some(tmux_pane) = lifecycle.tmux_pane.as_ref() else {
            return Ok(CaptureResponse::Failed(CaptureError::NotATmuxTarget));
        };
        if !lilo_runtime_platform::tmux::TmuxGateway::is_alive(tmux_pane).await? {
            return Ok(CaptureResponse::Failed(CaptureError::PaneUnavailable));
        }
        let scrollback_lines = request.scrollback_lines.unwrap_or(1000);
        Ok(
            match lilo_runtime_platform::tmux::TmuxGateway::capture_pane(
                tmux_pane,
                scrollback_lines,
            )
            .await
            {
                Ok(snapshot) => CaptureResponse::Captured(snapshot),
                Err(error) => CaptureResponse::Failed(error),
            },
        )
    }

    pub(crate) async fn record_shim_exit(&self, exit: ShimExit) -> Result<Option<RuntimeEvent>> {
        self.termination.record_shim_exit(self, exit).await
    }

    pub(crate) async fn status(&self, filter: StatusFilter) -> Vec<Lifecycle> {
        self.status.status(&self.store, &self.config, filter).await
    }

    pub(crate) async fn log_availability_statuses(&self) -> Vec<LifecycleLogAvailability> {
        self.status
            .log_availability_statuses(&self.store, &self.config)
            .await
    }

    pub(super) async fn populate_log_availability(&self, lifecycle: &mut Lifecycle) {
        self.status
            .populate_log_availability(&self.config, lifecycle)
            .await;
    }

    pub(crate) async fn events(
        &self,
        request: EventsRequest,
    ) -> std::result::Result<EventLogPage, CursorExpired> {
        self.events.events(request).await
    }

    pub(crate) async fn watcher_counts(&self) -> WatcherCounts {
        WatcherCounts {
            process_exit_watchers: self.watchers.process_exit_watcher_count().await,
            shim_sockets: self.spawn.pending_shim_socket_count().await,
            event_waiters: self.events.event_waiter_count(),
        }
    }

    pub(crate) async fn start_exit_watcher(
        self: &Arc<Self>,
        session_id: Uuid,
        runtime_pid: u32,
    ) -> Result<()> {
        self.watchers
            .start_exit_watcher(self, session_id, runtime_pid)
            .await
    }

    pub(crate) async fn is_terminal(&self, session_id: Uuid) -> bool {
        self.termination.is_terminal(&self.store, session_id).await
    }

    pub(super) async fn watcher_evidence(&self, session_id: Uuid) -> Result<TerminationEvidence> {
        self.termination
            .watcher_evidence(&self.store, session_id)
            .await
    }

    pub(super) async fn record_exited(
        &self,
        session_id: Uuid,
        exit: RuntimeExit,
        evidence: TerminationEvidence,
    ) -> Result<Option<RuntimeEvent>> {
        self.termination
            .record_exited(self, session_id, exit, evidence)
            .await
    }

    pub(crate) async fn record_lost(
        &self,
        session_id: Uuid,
        evidence: LostEvidence,
    ) -> Result<Option<RuntimeEvent>> {
        self.termination
            .record_lost(self, session_id, evidence)
            .await
    }

    pub(super) async fn remove_watcher(&self, session_id: Uuid) {
        self.watchers.remove_watcher(session_id).await;
    }

    pub(crate) async fn append_event(&self, event: RuntimeEvent) -> Result<RuntimeEvent> {
        self.events.append_event(event).await
    }
}
