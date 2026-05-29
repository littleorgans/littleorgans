use std::{sync::Arc, time::Duration};

use crate::backend::RuntimeBackends;
use crate::doctor;
use crate::server::ServerState;
use crate::service::RuntimeService;
use crate::spawn_preflight;
use anyhow::{Context, Result};
use lilo_rm_core::{
    CaptureRequest, CaptureResponse, DoctorResponse, EventBatch, EventsRequest, KillByPidRequest,
    KillByPidResponse, KillOutcome, KillRequest, Lifecycle, NudgeRequest, NudgeResponse,
    SpawnConflictPayload, SpawnRequest, SpawnedPayload, StatusFilter,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpawnOutcome {
    Spawned(SpawnedPayload),
    Conflict(SpawnConflictPayload),
}

/// Curated in-process runtime domain API reviewed under R1.
///
/// This is the public surface for co-located callers that need runtime behavior
/// without going through the socket RPC adapter. The methods intentionally mirror
/// runtime-owned verbs and return public payload types from `lilo_rm_core` or
/// standard library containers:
///
/// - `poll_events` returns `EventBatch`.
/// - `spawn` returns `SpawnOutcome`.
/// - `status` returns `Vec<Lifecycle>`.
/// - `kill_runtime` returns `KillOutcome`.
/// - `kill_by_pid` returns `KillByPidResponse`.
/// - `nudge_runtime` returns `NudgeResponse`.
/// - `capture` returns `CaptureResponse`.
/// - `doctor` returns `DoctorResponse`.
///
/// Session vocabulary (`reap_exited` / `terminate` / `watch_events` /
/// `terminate_all`) is NOT on `RuntimeService`; it lives on the WS2
/// `RuntimePort` and maps onto these verbs.
impl RuntimeService {
    pub async fn poll_events(&self, request: EventsRequest) -> EventBatch {
        poll_events_batch(self.state(), request).await
    }

    pub async fn spawn(&self, request: SpawnRequest) -> Result<SpawnOutcome> {
        spawn_domain(self.state(), request).await
    }

    pub async fn status(&self, filter: StatusFilter) -> Vec<Lifecycle> {
        status_domain(self.state(), filter).await
    }

    pub async fn kill_runtime(&self, request: KillRequest) -> Result<KillOutcome> {
        kill_runtime_domain(self.state(), request).await
    }

    pub async fn kill_by_pid(&self, request: KillByPidRequest) -> Result<KillByPidResponse> {
        kill_by_pid_domain(self.state(), request).await
    }

    pub async fn nudge_runtime(&self, request: NudgeRequest) -> Result<NudgeResponse> {
        nudge_runtime_domain(self.state(), request).await
    }

    pub async fn capture(&self, request: CaptureRequest) -> Result<CaptureResponse> {
        capture_domain(self.state(), request).await
    }

    pub async fn doctor(&self) -> Result<DoctorResponse> {
        doctor_domain(Arc::clone(self.state())).await
    }
}

pub(crate) async fn spawn_domain(
    state: &Arc<ServerState>,
    mut request: SpawnRequest,
) -> Result<SpawnOutcome> {
    if let Some(conflict) = spawn_preflight::check(state, &mut request).await? {
        return Ok(SpawnOutcome::Conflict(conflict));
    }
    let launch = lilo_runtime_launchers::dispatch(&request.runtime)?.launch_spec(&request)?;
    let backends = RuntimeBackends::new(state.config());
    let launch = backends.prepare_launch(&request, launch)?;
    let begin = state.begin_spawn(&request, launch.clone()).await?;
    let evidence = match backends.spawn(&request, &launch).await {
        Ok(evidence) => evidence,
        Err(error) => {
            state.cancel_spawn(request.session_id).await;
            return Err(error);
        }
    };

    let ready = tokio::time::timeout(Duration::from_secs(10), begin.ready)
        .await
        .context("timed out waiting for ShimReady")?
        .context("shim ready channel closed")?;
    let (lifecycle, event) = state
        .record_running(&request, ready, !begin.session_backed)
        .await?;
    let (log_dir, stdout_path, stderr_path) = match evidence.log_paths {
        Some(paths) => (
            Some(paths.log_dir),
            Some(paths.stdout_path),
            Some(paths.stderr_path),
        ),
        None => (None, None, None),
    };
    Ok(SpawnOutcome::Spawned(SpawnedPayload {
        lifecycle,
        event,
        log_dir,
        stdout_path,
        stderr_path,
    }))
}

pub(crate) async fn poll_events_batch(state: &ServerState, request: EventsRequest) -> EventBatch {
    match state.events(request).await {
        Ok(batch) => EventBatch::Events {
            events: batch.events,
            cursor: batch.cursor,
        },
        Err(expired) => EventBatch::CursorExpired {
            oldest: expired.oldest,
        },
    }
}

pub(crate) async fn status_domain(state: &ServerState, filter: StatusFilter) -> Vec<Lifecycle> {
    state.status(filter).await
}

pub(crate) async fn kill_runtime_domain(
    state: &ServerState,
    request: KillRequest,
) -> Result<KillOutcome> {
    state.kill_runtime(request).await
}

pub(crate) async fn kill_by_pid_domain(
    state: &ServerState,
    request: KillByPidRequest,
) -> Result<KillByPidResponse> {
    state.kill_pid(request).await
}

pub(crate) async fn nudge_runtime_domain(
    state: &ServerState,
    request: NudgeRequest,
) -> Result<NudgeResponse> {
    state.nudge_runtime(request).await
}

pub(crate) async fn capture_domain(
    state: &ServerState,
    request: CaptureRequest,
) -> Result<CaptureResponse> {
    state.capture_pane(request).await
}

pub(crate) async fn doctor_domain(state: Arc<ServerState>) -> Result<DoctorResponse> {
    doctor::collect(state).await
}

#[cfg(test)]
mod tests {
    use super::SpawnOutcome;
    use crate::test_support::RuntimeServiceFixture as ApiFixture;
    use crate::{ReconcileConfig, RuntimeService};
    use chrono::Utc;
    use lilo_im_core::Principal;
    use lilo_rm_core::{
        CaptureError, CaptureRequest, EventBatch, EventsRequest, HeadlessSpawnTarget,
        IsolationPolicy, KillByPidRequest, KillOutcome, KillRequest, Lifecycle, LifecycleState,
        NudgeFailureReason, NudgeOutcome, NudgeRequest, RuntimeEvent, RuntimeKind, RuntimeResponse,
        RuntimeRpc, RuntimeSignal, ShimReady, SpawnConflictKind, SpawnConflictPayload,
        SpawnRequest, SpawnTarget, SpawnedPayload, StatusFilter, StatusRequest,
    };
    use std::path::Path;
    use std::process::Command;
    use std::sync::Arc;

    // Outside pid_t range, so shutdown drain cannot signal an unrelated CI process.
    const TEST_SHIM_PID: u32 = u32::MAX;
    use std::time::Duration;
    use uuid::Uuid;

    #[tokio::test]
    async fn domain_surface_verbs_match_wire_responses() {
        let fixture = ApiFixture::new(ReconcileConfig::default()).await;
        let service = RuntimeService::build(fixture.context())
            .await
            .expect("service builds");

        assert_status_parity(&service).await;
        assert_kill_runtime_parity(&service).await;
        assert_kill_by_pid_parity(&service).await;
        assert_nudge_parity(&service).await;
        assert_capture_parity(&service).await;
        assert_doctor_parity(&service).await;

        service.shutdown().await.expect("shutdown succeeds");
        fixture.db.close().await;
    }

    async fn assert_status_parity(service: &RuntimeService) {
        let direct_status = service.status(StatusFilter::default()).await;
        let wire_status = service
            .handle_rpc(
                local_principal(),
                RuntimeRpc::Status {
                    request: StatusRequest {
                        session_id: None,
                        session_ids: Vec::new(),
                        updated_since: None,
                        runtime: None,
                        state: None,
                    },
                },
            )
            .await;
        let RuntimeResponse::Status(status_payload) = wire_status else {
            panic!("expected status response, got {wire_status:?}");
        };
        assert_eq!(direct_status, status_payload.lifecycles);
    }

    async fn assert_kill_runtime_parity(service: &RuntimeService) {
        let kill_session_id = Uuid::now_v7();
        let kill_pid = finished_child_pid();
        insert_running_headless(service, kill_session_id, kill_pid).await;
        let kill_request = KillRequest {
            session_id: kill_session_id,
            signal: RuntimeSignal::Term,
            grace_secs: 0,
        };
        let direct_kill = service
            .kill_runtime(kill_request.clone())
            .await
            .expect("domain kill succeeds");
        let wire_kill = service
            .handle_rpc(
                local_principal(),
                RuntimeRpc::Kill {
                    request: kill_request,
                },
            )
            .await;
        let RuntimeResponse::Killed(kill_payload) = wire_kill else {
            panic!("expected kill response, got {wire_kill:?}");
        };
        assert_eq!(direct_kill, KillOutcome::AlreadyExited);
        assert_eq!(direct_kill, kill_payload.outcome);
    }

    async fn assert_kill_by_pid_parity(service: &RuntimeService) {
        let kill_by_pid_request = KillByPidRequest {
            pid: finished_child_pid(),
            signal: lilo_runtime_platform::signal::signal_number(RuntimeSignal::Term),
            grace_secs: 0,
        };
        let direct_kill_by_pid = service
            .kill_by_pid(kill_by_pid_request.clone())
            .await
            .expect("domain kill by pid succeeds");
        let wire_kill_by_pid = service
            .handle_rpc(
                local_principal(),
                RuntimeRpc::KillByPid {
                    request: kill_by_pid_request,
                },
            )
            .await;
        let RuntimeResponse::KillByPid(kill_by_pid_payload) = wire_kill_by_pid else {
            panic!("expected kill by pid response, got {wire_kill_by_pid:?}");
        };
        assert_eq!(direct_kill_by_pid, kill_by_pid_payload.response);
    }

    async fn assert_nudge_parity(service: &RuntimeService) {
        let nudge_session_id = Uuid::now_v7();
        insert_running_headless(service, nudge_session_id, finished_child_pid()).await;
        let nudge_request = NudgeRequest {
            session_id: nudge_session_id,
            content: "wake".to_owned(),
        };
        let direct_nudge = service
            .nudge_runtime(nudge_request.clone())
            .await
            .expect("domain nudge succeeds");
        let wire_nudge = service
            .handle_rpc(
                local_principal(),
                RuntimeRpc::Nudge {
                    request: nudge_request,
                },
            )
            .await;
        let RuntimeResponse::Nudge(nudge_payload) = wire_nudge else {
            panic!("expected nudge response, got {wire_nudge:?}");
        };
        assert_eq!(direct_nudge, nudge_payload.response);
        assert_eq!(
            direct_nudge.outcome,
            NudgeOutcome::Unsupported(NudgeFailureReason::HeadlessLifecycle)
        );
    }

    async fn assert_capture_parity(service: &RuntimeService) {
        let capture_request = CaptureRequest {
            session_id: Uuid::now_v7(),
            scrollback_lines: None,
        };
        let direct_capture = service
            .capture(capture_request.clone())
            .await
            .expect("domain capture succeeds");
        let wire_capture = service
            .handle_rpc(
                local_principal(),
                RuntimeRpc::Capture {
                    request: capture_request,
                },
            )
            .await;
        let RuntimeResponse::Capture(capture_payload) = wire_capture else {
            panic!("expected capture response, got {wire_capture:?}");
        };
        assert_eq!(direct_capture, capture_payload.response);
        assert_eq!(
            direct_capture.into_result(),
            Err(CaptureError::SessionMissing)
        );
    }

    async fn assert_doctor_parity(service: &RuntimeService) {
        let direct_doctor = service.doctor().await.expect("domain doctor succeeds");
        let wire_doctor = service
            .handle_rpc(local_principal(), RuntimeRpc::Doctor)
            .await;
        let RuntimeResponse::Doctor(doctor_payload) = wire_doctor else {
            panic!("expected doctor response, got {wire_doctor:?}");
        };
        assert_eq!(direct_doctor.version, doctor_payload.doctor.version);
        assert_eq!(direct_doctor.socket_path, doctor_payload.doctor.socket_path);
        assert_eq!(direct_doctor.lifecycles, doctor_payload.doctor.lifecycles);
    }

    fn local_principal() -> Principal {
        Principal::local(nix::unistd::getuid().as_raw())
    }

    #[tokio::test]
    async fn spawn_domain_matches_wire_spawn_structure() {
        let fixture = ApiFixture::new(ReconcileConfig::default()).await;
        let service = RuntimeService::build(fixture.context())
            .await
            .expect("service builds");
        let runtime_pid = std::process::id();
        let direct = spawn_direct_with_ready(
            &service,
            spawn_request(Uuid::now_v7(), fixture.dir.path()),
            runtime_pid,
        )
        .await;
        let wire = spawn_wire_with_ready(
            &service,
            spawn_request(Uuid::now_v7(), fixture.dir.path()),
            runtime_pid,
        )
        .await;

        assert_spawned_payload_parity(
            &expect_spawned(direct),
            &expect_wire_spawned(wire),
            runtime_pid,
            TEST_SHIM_PID,
        );
        service.shutdown().await.expect("shutdown succeeds");
        fixture.db.close().await;
    }

    #[tokio::test]
    async fn spawn_domain_and_wire_report_same_id_conflicts() {
        let fixture = ApiFixture::new(ReconcileConfig::default()).await;
        let service = RuntimeService::build(fixture.context())
            .await
            .expect("service builds");
        let runtime_pid = std::process::id();
        let direct_request = spawn_request(Uuid::now_v7(), fixture.dir.path());
        let wire_request = spawn_request(Uuid::now_v7(), fixture.dir.path());
        let _ = spawn_direct_with_ready(&service, direct_request.clone(), runtime_pid).await;
        let _ = spawn_wire_with_ready(&service, wire_request.clone(), runtime_pid).await;

        let direct_conflict = service
            .spawn(direct_request)
            .await
            .expect("direct conflict response");
        let wire_conflict = service
            .handle_rpc(
                Principal::local(nix::unistd::getuid().as_raw()),
                RuntimeRpc::Spawn {
                    request: wire_request,
                },
            )
            .await;

        assert_conflict_parity(
            &expect_conflict(direct_conflict),
            &expect_wire_conflict(wire_conflict),
        );
        service.shutdown().await.expect("shutdown succeeds");
        fixture.db.close().await;
    }

    #[tokio::test]
    async fn poll_events_matches_wire_events_response() {
        let fixture = ApiFixture::new(ReconcileConfig::default()).await;
        let service = RuntimeService::build(fixture.context())
            .await
            .expect("service builds");
        let event = RuntimeEvent::Running {
            session_id: Uuid::now_v7(),
            runtime_pid: 4242,
            start_time: Utc::now(),
        };
        service
            .append_event(event.clone())
            .await
            .expect("event appended");

        let request = EventsRequest::default();
        let direct = service.poll_events(request).await;
        let wire = service
            .handle_rpc(
                Principal::local(nix::unistd::getuid().as_raw()),
                RuntimeRpc::Events { request },
            )
            .await;

        match (direct, wire) {
            (EventBatch::Events { events, cursor }, RuntimeResponse::Events(payload)) => {
                assert_eq!(events, vec![event]);
                assert_eq!(events, payload.events);
                assert_eq!(cursor, payload.cursor);
            }
            other => panic!("expected matching event batches, got {other:?}"),
        }

        service.shutdown().await.expect("shutdown succeeds");
        fixture.db.close().await;
    }

    async fn spawn_direct_with_ready(
        service: &RuntimeService,
        request: SpawnRequest,
        runtime_pid: u32,
    ) -> SpawnOutcome {
        let ready =
            complete_ready_after_wait(Arc::clone(service.state()), request.session_id, runtime_pid);
        let (outcome, ready) = tokio::join!(service.spawn(request), ready);
        ready.expect("shim ready accepted");
        outcome.expect("spawn succeeds")
    }

    async fn spawn_wire_with_ready(
        service: &RuntimeService,
        request: SpawnRequest,
        runtime_pid: u32,
    ) -> RuntimeResponse {
        let ready =
            complete_ready_after_wait(Arc::clone(service.state()), request.session_id, runtime_pid);
        let response = service.handle_rpc(
            Principal::local(nix::unistd::getuid().as_raw()),
            RuntimeRpc::Spawn { request },
        );
        let (response, ready) = tokio::join!(response, ready);
        ready.expect("shim ready accepted");
        response
    }

    async fn complete_ready_after_wait(
        state: Arc<crate::server::ServerState>,
        session_id: Uuid,
        runtime_pid: u32,
    ) -> anyhow::Result<()> {
        tokio::time::sleep(Duration::from_millis(25)).await;
        state
            .complete_shim_ready(ShimReady {
                session_id,
                shim_pid: TEST_SHIM_PID,
                runtime_pid,
                start_time: Utc::now(),
                tmux_pane: None,
            })
            .await
    }

    async fn insert_running_headless(service: &RuntimeService, session_id: Uuid, runtime_pid: u32) {
        let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
        service
            .state()
            .store()
            .insert_forking(&lifecycle)
            .await
            .expect("insert forking lifecycle");
        lifecycle.mark_running(ShimReady {
            session_id,
            shim_pid: TEST_SHIM_PID,
            runtime_pid,
            start_time: Utc::now(),
            tmux_pane: None,
        });
        service
            .state()
            .store()
            .update_lifecycle(&lifecycle)
            .await
            .expect("update running lifecycle");
    }

    fn finished_child_pid() -> u32 {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("exit 0")
            .spawn()
            .expect("spawn short child");
        let pid = child.id();
        child.wait().expect("child exits");
        pid
    }

    fn assert_spawned_payload_parity(
        direct: &SpawnedPayload,
        wire: &SpawnedPayload,
        runtime_pid: u32,
        shim_pid: u32,
    ) {
        for payload in [direct, wire] {
            assert_eq!(payload.lifecycle.state, LifecycleState::Running);
            assert_eq!(payload.lifecycle.runtime_pid, Some(runtime_pid));
            assert_eq!(payload.lifecycle.shim_pid, Some(shim_pid));
            assert_running_event(&payload.event, payload.lifecycle.session_id, runtime_pid);
        }
        assert_eq!(direct.log_dir.is_some(), wire.log_dir.is_some());
        assert_eq!(direct.stdout_path.is_some(), wire.stdout_path.is_some());
        assert_eq!(direct.stderr_path.is_some(), wire.stderr_path.is_some());
        assert_eq!(
            direct
                .stdout_path
                .as_ref()
                .and_then(|path| path.file_name()),
            wire.stdout_path.as_ref().and_then(|path| path.file_name())
        );
        assert_eq!(
            direct
                .stderr_path
                .as_ref()
                .and_then(|path| path.file_name()),
            wire.stderr_path.as_ref().and_then(|path| path.file_name())
        );
    }

    fn assert_running_event(event: &RuntimeEvent, session_id: Uuid, runtime_pid: u32) {
        let RuntimeEvent::Running {
            session_id: event_session_id,
            runtime_pid: event_runtime_pid,
            ..
        } = event
        else {
            panic!("expected running event, got {event:?}");
        };
        assert_eq!(*event_session_id, session_id);
        assert_eq!(*event_runtime_pid, runtime_pid);
    }

    fn assert_conflict_parity(direct: &SpawnConflictPayload, wire: &SpawnConflictPayload) {
        assert_eq!(direct.kind, SpawnConflictKind::SessionId);
        assert_eq!(wire.kind, SpawnConflictKind::SessionId);
        assert_eq!(direct.lifecycle.state, LifecycleState::Running);
        assert_eq!(wire.lifecycle.state, LifecycleState::Running);
    }

    fn expect_spawned(outcome: SpawnOutcome) -> SpawnedPayload {
        let SpawnOutcome::Spawned(payload) = outcome else {
            panic!("expected spawned outcome, got {outcome:?}");
        };
        payload
    }

    fn expect_wire_spawned(response: RuntimeResponse) -> SpawnedPayload {
        let RuntimeResponse::Spawned(payload) = response else {
            panic!("expected spawned response, got {response:?}");
        };
        payload
    }

    fn expect_conflict(outcome: SpawnOutcome) -> SpawnConflictPayload {
        let SpawnOutcome::Conflict(payload) = outcome else {
            panic!("expected conflict outcome, got {outcome:?}");
        };
        payload
    }

    fn expect_wire_conflict(response: RuntimeResponse) -> SpawnConflictPayload {
        let RuntimeResponse::SpawnConflict(payload) = response else {
            panic!("expected conflict response, got {response:?}");
        };
        payload
    }

    fn spawn_request(session_id: Uuid, cwd: &Path) -> SpawnRequest {
        SpawnRequest {
            session_id,
            runtime: RuntimeKind::Claude,
            isolation: IsolationPolicy::default(),
            image: None,
            env: Vec::new(),
            mounts: Vec::new(),
            cwd: cwd.to_path_buf(),
            target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
            force: false,
            shell_resume: None,
        }
    }
}
