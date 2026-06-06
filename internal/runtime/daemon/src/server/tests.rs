use super::*;
use lilo_common::id::SessionId;

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn nudge_terminal_tmux_session_returns_typed_failure() {
    assert_terminal_tmux_nudge_returns_session_ended(TerminalState::Exited).await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn nudge_lost_tmux_session_returns_terminal_failure() {
    assert_terminal_tmux_nudge_returns_session_ended(TerminalState::Lost).await;
}

async fn assert_terminal_tmux_nudge_returns_session_ended(terminal_state: TerminalState) {
    let state = TestState::new().await;
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    persist_terminal_tmux(&state.server, session_id, terminal_state).await;

    let response = state
        .server
        .nudge_runtime(nudge_request(session_id))
        .await
        .expect("nudge");

    assert_eq!(
        response,
        NudgeResponse {
            delivered: false,
            outcome: NudgeOutcome::Failed(NudgeFailureReason::SessionEnded),
        }
    );
    state.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn nudge_headless_terminal_session_remains_headless_unsupported() {
    let state = TestState::new().await;
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let mut lifecycle = Lifecycle::forking(session_id, lilo_rm_core::RuntimeKind::Claude);
    state
        .server
        .store()
        .insert_forking(&lifecycle)
        .await
        .expect("insert");
    lifecycle.mark_exited(RuntimeExit::new(Some(0), None));
    state
        .server
        .store()
        .update_lifecycle(&lifecycle)
        .await
        .expect("terminal");

    let response = state
        .server
        .nudge_runtime(nudge_request(session_id))
        .await
        .expect("nudge");

    assert_eq!(
        response,
        NudgeResponse {
            delivered: false,
            outcome: NudgeOutcome::Unsupported(NudgeFailureReason::HeadlessLifecycle),
        }
    );
    state.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn nudge_running_tmux_session_with_dead_pane_returns_typed_failure() {
    let state = TestState::new().await;
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    persist_running_tmux(&state.server, session_id).await;

    let response = state
        .server
        .nudge_runtime(nudge_request(session_id))
        .await
        .expect("nudge");

    assert_eq!(
        response,
        NudgeResponse {
            delivered: false,
            outcome: NudgeOutcome::Failed(NudgeFailureReason::TmuxPaneDead),
        }
    );
    state.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn capture_running_tmux_session_with_dead_pane_returns_pane_unavailable() {
    let state = TestState::new().await;
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    persist_running_tmux(&state.server, session_id).await;

    let response = state
        .server
        .capture_pane(CaptureRequest {
            session_id,
            scrollback_lines: None,
        })
        .await
        .expect("capture");

    assert_eq!(
        response,
        CaptureResponse::Failed(CaptureError::PaneUnavailable)
    );
    state.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn status_marks_dead_tmux_pane_logs_unavailable() {
    let state = TestState::new().await;
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    persist_running_tmux(&state.server, session_id).await;

    let lifecycles = state
        .server
        .status(StatusFilter {
            session_id: Some(session_id),
            ..StatusFilter::empty()
        })
        .await;

    assert_eq!(
        lifecycles[0].log_availability,
        Some(LogAvailability::Unavailable {
            reason: LogsUnavailableReason::PaneUnavailable,
        })
    );
    state.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn kill_unknown_session_returns_not_found() {
    let state = TestState::new().await;
    let request = KillRequest {
        session_id: SessionId::from_uuid(uuid::Uuid::now_v7()),
        signal: RuntimeSignal::Term,
        grace_secs: 0,
    };

    let error = state
        .server
        .kill_runtime(request)
        .await
        .expect_err("not found");
    assert!(error.to_string().contains("not found"), "{error}");
    state.cleanup().await;
}

enum TerminalState {
    Exited,
    Lost,
}

struct TestState {
    server: ServerState,
    testdb: lilo_db::test_support::TestDb,
    _temp: tempfile::TempDir,
}

impl TestState {
    async fn new() -> Self {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let store_config = StoreConfig {
            db_path: temp.path().join("rtm.sqlite"),
        };
        let testdb = lilo_db::test_support::TestDb::create()
            .await
            .expect("store db");
        let store = LifecycleStore::from_db(testdb.db());
        let server = ServerState::new(
            testdb.db(),
            DaemonConfig {
                endpoint: lilo_paths::RuntimeEndpoint::unix_socket("/tmp/rtm-test.sock"),
                shim_path: PathBuf::from("rtm"),
                log_root: temp.path().join("logs"),
                store: store_config,
                reconcile: reconcile::ReconcileConfig::default(),
                docker_preflight: crate::docker_preflight::DockerPreflightConfig::default(),
                tmux_server_label: None,
            },
            store,
        )
        .expect("state");
        Self {
            server,
            testdb,
            _temp: temp,
        }
    }

    async fn cleanup(self) {
        self.testdb.cleanup().await.expect("test db cleans up");
    }
}

async fn persist_terminal_tmux(
    state: &ServerState,
    session_id: SessionId,
    terminal_state: TerminalState,
) {
    let mut lifecycle = persist_running_tmux(state, session_id).await;
    match terminal_state {
        TerminalState::Exited => {
            lifecycle.mark_exited(RuntimeExit::new(Some(0), None));
        }
        TerminalState::Lost => {
            lifecycle.mark_lost(LostEvidence::PidNotAlive);
        }
    }
    state
        .store()
        .update_lifecycle(&lifecycle)
        .await
        .expect("terminal");
}

async fn persist_running_tmux(state: &ServerState, session_id: SessionId) -> Lifecycle {
    let mut lifecycle = Lifecycle::forking(session_id, lilo_rm_core::RuntimeKind::Claude);
    state
        .store()
        .insert_forking(&lifecycle)
        .await
        .expect("insert");
    lifecycle.mark_running(ShimReady {
        session_id,
        shim_pid: 100,
        runtime_pid: 200,
        start_time: chrono::Utc::now(),
        tmux_pane: Some("rtm-missing:0.1".parse().expect("tmux pane")),
    });
    state
        .store()
        .update_lifecycle(&lifecycle)
        .await
        .expect("running");
    lifecycle
}

fn nudge_request(session_id: SessionId) -> NudgeRequest {
    NudgeRequest {
        session_id,
        content: "wake up".to_owned(),
        mode: lilo_rm_core::NudgeMode::Immediate,
        timeout_ms: None,
    }
}
