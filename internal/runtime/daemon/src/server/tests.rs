use super::*;

#[tokio::test]
async fn nudge_terminal_tmux_session_returns_typed_failure() {
    let state = TestState::new().await;
    let session_id = Uuid::now_v7();
    persist_terminal_tmux(&state.server, session_id, TerminalState::Exited).await;

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
}

#[tokio::test]
async fn nudge_lost_tmux_session_returns_terminal_failure() {
    let state = TestState::new().await;
    let session_id = Uuid::now_v7();
    persist_terminal_tmux(&state.server, session_id, TerminalState::Lost).await;

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
}

#[tokio::test]
async fn nudge_headless_terminal_session_remains_headless_unsupported() {
    let state = TestState::new().await;
    let session_id = Uuid::now_v7();
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
}

#[tokio::test]
async fn nudge_running_tmux_session_with_dead_pane_returns_typed_failure() {
    let state = TestState::new().await;
    let session_id = Uuid::now_v7();
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
}

#[tokio::test]
async fn capture_running_tmux_session_with_dead_pane_returns_pane_unavailable() {
    let state = TestState::new().await;
    let session_id = Uuid::now_v7();
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
}

#[tokio::test]
async fn status_marks_dead_tmux_pane_logs_unavailable() {
    let state = TestState::new().await;
    let session_id = Uuid::now_v7();
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
}

#[tokio::test]
async fn kill_unknown_session_returns_not_found() {
    let state = TestState::new().await;
    let request = KillRequest {
        session_id: Uuid::now_v7(),
        signal: RuntimeSignal::Term,
        grace_secs: 0,
    };

    let error = state
        .server
        .kill_runtime(request)
        .await
        .expect_err("not found");
    assert!(error.to_string().contains("not found"), "{error}");
}

enum TerminalState {
    Exited,
    Lost,
}

struct TestState {
    server: ServerState,
    _temp: tempfile::TempDir,
}

impl TestState {
    async fn new() -> Self {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let store_config = StoreConfig {
            db_path: temp.path().join("rtm.sqlite"),
        };
        let db = lilo_db::LiloDb::open_path(&store_config.db_path)
            .await
            .expect("store db");
        let store = LifecycleStore::open(&db);
        let server = ServerState::new(
            DaemonConfig {
                endpoint: lilo_paths::RuntimeEndpoint::unix_socket("/tmp/rtm-test.sock"),
                shim_path: PathBuf::from("rtm"),
                log_root: temp.path().join("logs"),
                store: store_config,
                reconcile: reconcile::ReconcileConfig::default(),
                docker_preflight: crate::docker_preflight::DockerPreflightConfig::default(),
            },
            store,
        )
        .expect("state");
        Self {
            server,
            _temp: temp,
        }
    }
}

async fn persist_terminal_tmux(
    state: &ServerState,
    session_id: Uuid,
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

async fn persist_running_tmux(state: &ServerState, session_id: Uuid) -> Lifecycle {
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

fn nudge_request(session_id: Uuid) -> NudgeRequest {
    NudgeRequest {
        session_id,
        content: "wake up".to_owned(),
    }
}
