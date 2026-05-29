use std::{sync::Arc, time::Duration};

use crate::backend::RuntimeBackends;
use crate::handler;
use crate::server::{
    DaemonConfig, ServerState, prepare_runtime_bootstrap, start_runtime_reconcile,
};
use crate::spawn_preflight;
use anyhow::{Context, Result};
use lilo_db::LiloDb;
use lilo_im_core::Principal;
use lilo_rm_core::{
    EventBatch, EventsRequest, RuntimeEvent, RuntimeResponse, RuntimeRpc, SpawnConflictPayload,
    SpawnRequest, SpawnedPayload,
};
use tokio::sync::{Mutex, broadcast};
use tokio::task::JoinHandle;

pub struct RuntimeServiceContext {
    config: DaemonConfig,
    db: LiloDb,
    local_uid: u32,
}

impl RuntimeServiceContext {
    pub fn new(config: DaemonConfig, db: LiloDb) -> Self {
        Self::new_with_local_uid(config, db, nix::unistd::getuid().as_raw())
    }

    pub fn new_with_local_uid(config: DaemonConfig, db: LiloDb, local_uid: u32) -> Self {
        Self {
            config,
            db,
            local_uid,
        }
    }

    pub async fn from_env() -> Result<Self> {
        let config = DaemonConfig::from_env()?;
        let db = LiloDb::open_path(&config.store.db_path).await?;
        Ok(Self::new(config, db))
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub fn into_parts(self) -> (DaemonConfig, LiloDb, u32) {
        (self.config, self.db, self.local_uid)
    }
}

pub struct RuntimeService {
    config: DaemonConfig,
    state: Arc<ServerState>,
    shutdown_tx: broadcast::Sender<()>,
    reconcile_task: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpawnOutcome {
    Spawned(SpawnedPayload),
    Conflict(SpawnConflictPayload),
}

impl RuntimeService {
    pub async fn build(ctx: RuntimeServiceContext) -> Result<Self> {
        let (config, db, local_uid) = ctx.into_parts();
        let bootstrap = prepare_runtime_bootstrap(&config, &db, local_uid)?;
        let state = bootstrap.into_state(config.clone())?;
        let reconcile = start_runtime_reconcile(Arc::clone(&state), config.reconcile).await?;
        Ok(Self {
            config,
            state,
            shutdown_tx: reconcile.shutdown_tx,
            reconcile_task: Mutex::new(Some(reconcile.reconcile_task)),
        })
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub async fn handle_rpc(&self, principal: Principal, rpc: RuntimeRpc) -> RuntimeResponse {
        let response = handler::handle_rpc(principal, rpc, Arc::clone(&self.state)).await;
        if matches!(response, RuntimeResponse::Stopping) {
            let _ = self.shutdown_tx.send(());
        }
        response
    }

    pub async fn poll_events(&self, request: EventsRequest) -> EventBatch {
        poll_events_batch(&self.state, request).await
    }

    pub async fn spawn(&self, request: SpawnRequest) -> Result<SpawnOutcome> {
        spawn_domain(&self.state, request).await
    }

    pub async fn append_event(&self, event: RuntimeEvent) -> Result<RuntimeEvent> {
        self.state.append_event(event).await
    }

    pub fn subscribe_shutdown(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    pub async fn shutdown(&self) -> Result<()> {
        let _ = self.shutdown_tx.send(());
        self.state.drain_shims();
        let reconcile_task = {
            let mut task = self.reconcile_task.lock().await;
            task.take()
        };
        if let Some(task) = reconcile_task {
            task.await.context("periodic reconciliation task failed")?;
        }
        Ok(())
    }

    /// Reap shims spawned by this service. Public so in-process owners (the
    /// session daemon, test harnesses) can drain without a full async shutdown.
    pub fn drain_shims(&self) {
        self.state.drain_shims();
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

impl Drop for RuntimeService {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        // Catch-all so shims never outlive an owner that dropped the service
        // without an explicit shutdown (e.g. a test harness with no teardown).
        self.state.drain_shims();
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeService, RuntimeServiceContext, SpawnOutcome};
    use crate::{DaemonConfig, ReconcileConfig, docker_preflight::DockerPreflightConfig};
    use chrono::Utc;
    use lilo_db::LiloDb;
    use lilo_im_core::Principal;
    use lilo_paths::{LiloHome, LiloPaths};
    use lilo_rm_core::{
        EventBatch, EventsRequest, HeadlessSpawnTarget, IsolationPolicy, LifecycleState,
        RuntimeEvent, RuntimeKind, RuntimeResponse, RuntimeRpc, ShimReady, SpawnConflictKind,
        SpawnConflictPayload, SpawnRequest, SpawnTarget, SpawnedPayload,
    };
    use lilo_runtime_store::StoreConfig;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;
    use uuid::Uuid;

    #[tokio::test]
    async fn build_preserves_daemon_config_for_later_composition() {
        let fixture = ServiceFixture::new(ReconcileConfig::default()).await;

        let service = RuntimeService::build(fixture.context())
            .await
            .expect("service builds");

        assert_eq!(
            service.config().socket_path().expect("socket"),
            fixture.config.socket_path().expect("socket")
        );
    }

    #[tokio::test]
    async fn runtime_shutdown_drains_periodic_reconcile_task() {
        let fixture = ServiceFixture::new(ReconcileConfig {
            sweep_interval: Duration::from_mins(1),
            resume_poll_interval: Duration::from_mins(1),
            ..ReconcileConfig::default()
        })
        .await;
        let service = RuntimeService::build(fixture.context())
            .await
            .expect("service builds");

        tokio::time::timeout(Duration::from_millis(100), service.shutdown())
            .await
            .expect("shutdown returns before timeout")
            .expect("shutdown succeeds");
        service.shutdown().await.expect("second shutdown succeeds");
        fixture.db.close().await;
    }

    #[tokio::test]
    async fn spawn_domain_matches_wire_spawn_structure() {
        let fixture = ServiceFixture::new(ReconcileConfig::default()).await;
        let service = RuntimeService::build(fixture.context())
            .await
            .expect("service builds");
        let runtime_pid = std::process::id();
        let direct = spawn_direct_with_ready(
            &service,
            spawn_request(Uuid::now_v7(), fixture._dir.path()),
            runtime_pid,
        )
        .await;
        let wire = spawn_wire_with_ready(
            &service,
            spawn_request(Uuid::now_v7(), fixture._dir.path()),
            runtime_pid,
        )
        .await;

        assert_spawned_payload_parity(
            expect_spawned(direct),
            expect_wire_spawned(wire),
            runtime_pid,
        );
        service.shutdown().await.expect("shutdown succeeds");
        fixture.db.close().await;
    }

    #[tokio::test]
    async fn spawn_domain_and_wire_report_same_id_conflicts() {
        let fixture = ServiceFixture::new(ReconcileConfig::default()).await;
        let service = RuntimeService::build(fixture.context())
            .await
            .expect("service builds");
        let runtime_pid = std::process::id();
        let direct_request = spawn_request(Uuid::now_v7(), fixture._dir.path());
        let wire_request = spawn_request(Uuid::now_v7(), fixture._dir.path());
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
            expect_conflict(direct_conflict),
            expect_wire_conflict(wire_conflict),
        );
        service.shutdown().await.expect("shutdown succeeds");
        fixture.db.close().await;
    }

    #[tokio::test]
    async fn poll_events_matches_wire_events_response() {
        let fixture = ServiceFixture::new(ReconcileConfig::default()).await;
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
            complete_ready_after_wait(Arc::clone(&service.state), request.session_id, runtime_pid);
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
            complete_ready_after_wait(Arc::clone(&service.state), request.session_id, runtime_pid);
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
                shim_pid: runtime_pid.saturating_add(1),
                runtime_pid,
                start_time: Utc::now(),
                tmux_pane: None,
            })
            .await
    }

    fn assert_spawned_payload_parity(
        direct: SpawnedPayload,
        wire: SpawnedPayload,
        runtime_pid: u32,
    ) {
        for payload in [&direct, &wire] {
            assert_eq!(payload.lifecycle.state, LifecycleState::Running);
            assert_eq!(payload.lifecycle.runtime_pid, Some(runtime_pid));
            assert_eq!(
                payload.lifecycle.shim_pid,
                Some(runtime_pid.saturating_add(1))
            );
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

    fn assert_conflict_parity(direct: SpawnConflictPayload, wire: SpawnConflictPayload) {
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

    fn install_fake_shim(path: &Path) {
        std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("fake shim");
        let mut permissions = std::fs::metadata(path)
            .expect("fake shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("fake shim permissions");
    }

    struct ServiceFixture {
        _dir: tempfile::TempDir,
        config: DaemonConfig,
        db: LiloDb,
    }

    impl ServiceFixture {
        async fn new(reconcile: ReconcileConfig) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let paths = LiloPaths::new(LiloHome::from_path(dir.path().join("lilo")).expect("home"));
            let config = DaemonConfig {
                endpoint: lilo_paths::RuntimeEndpoint::unix_socket(paths.socket_path()),
                shim_path: dir.path().join("shim"),
                log_root: paths.logs_root(),
                store: StoreConfig {
                    db_path: paths.db_path(),
                },
                reconcile,
                docker_preflight: DockerPreflightConfig::default(),
            };
            install_fake_shim(&config.shim_path);
            let db = LiloDb::open(&paths).await.expect("db");

            Self {
                _dir: dir,
                config,
                db,
            }
        }

        fn context(&self) -> RuntimeServiceContext {
            RuntimeServiceContext::new(self.config.clone(), self.db.clone())
        }
    }
}
