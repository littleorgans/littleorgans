use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use lilo_rm_core::{EventBatch, EventCursor, EventsRequest, StatusFilter};

use crate::background_task::BackgroundTask;
use crate::handler::DaemonState;

const EVENT_WAIT_MS: u32 = 30_000;
const EVENT_ERROR_RETRY: Duration = Duration::from_millis(500);

pub struct RuntimeEventTask {
    task: BackgroundTask,
}

impl RuntimeEventTask {
    pub fn spawn(state: Arc<DaemonState>) -> Self {
        let task = BackgroundTask::spawn(async move {
            if let Err(error) = run_event_loop(state).await {
                tracing::warn!(error = ?error, "runtime event loop stopped");
            }
        });

        Self { task }
    }

    pub async fn shutdown(&self) {
        self.task.shutdown().await;
    }
}

async fn run_event_loop(state: Arc<DaemonState>) -> Result<()> {
    let mut cursor = state
        .store()
        .event_cursor()
        .await
        .context("failed to load runtime event cursor")?;

    loop {
        let batch = state
            .runtime
            .poll_events(EventsRequest {
                since: cursor,
                wait_ms: Some(EVENT_WAIT_MS),
            })
            .await
            .context("failed to poll runtime events")?;

        if let Err(error) = handle_batch(&state, &mut cursor, batch).await {
            tracing::warn!(
                error = ?error,
                "failed to process runtime event batch; retrying without advancing cursor"
            );
            tokio::time::sleep(EVENT_ERROR_RETRY).await;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchOutcome {
    Advanced,
    Reconciled,
}

pub(crate) async fn handle_batch(
    state: &DaemonState,
    cursor: &mut Option<EventCursor>,
    batch: EventBatch,
) -> Result<BatchOutcome> {
    match batch {
        EventBatch::Events {
            events,
            cursor: next,
        } => {
            state
                .store()
                .apply_runtime_events_and_cursor(&events, next)
                .await
                .context("failed to persist runtime events")?;
            *cursor = Some(next);
            Ok(BatchOutcome::Advanced)
        }
        EventBatch::CursorExpired { oldest } => {
            let lifecycles = state
                .runtime
                .status(StatusFilter::empty())
                .await
                .context("failed to reconcile expired runtime cursor")?;
            crate::reconcile::reconcile_lifecycles(state, &lifecycles).await?;
            state
                .store()
                .apply_cursor(oldest)
                .await
                .context("failed to persist expired runtime cursor")?;
            *cursor = Some(oldest);
            Ok(BatchOutcome::Reconciled)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::OrPanic as _;
    use std::path::PathBuf;

    use chrono::Utc;
    use lilo_db::LiloDb;
    use lilo_paths::{LiloHome, LiloPaths};
    use lilo_rm_core::{
        IsolationPolicy, Lifecycle, LifecycleState, LostEvidence, RuntimeEvent, RuntimeKind,
        TerminationEvidence,
    };
    use lilo_runtime_daemon::{DaemonConfig, RuntimeService, RuntimeServiceContext};
    use lilo_runtime_store::LifecycleStore;
    use lilo_session_core::{
        Label, Namespace, RuntimeKind as SmRuntimeKind, Session, SessionState,
    };
    use lilo_session_driver::InProcessRuntime;
    use lilo_session_store::SqliteStore;
    use uuid::Uuid;

    use crate::identity_client::IdentityClient;

    use super::*;

    struct TestState {
        daemon: DaemonState,
        runtime_lifecycles: LifecycleStore,
    }

    #[tokio::test]
    async fn handle_batch_applies_events_and_advances_cursor() {
        let test = test_state().await;
        let state = &test.daemon;
        let running = insert_session(state, SessionState::Spawning).await;
        let terminated = insert_session(state, SessionState::Running).await;
        let lost = insert_session(state, SessionState::Running).await;
        let mut cursor = None;

        let outcome = handle_batch(
            state,
            &mut cursor,
            EventBatch::Events {
                events: vec![
                    RuntimeEvent::Running {
                        session_id: running,
                        runtime_pid: 101,
                        start_time: Utc::now(),
                    },
                    RuntimeEvent::Terminated {
                        session_id: terminated,
                        exit_code: Some(7),
                        signal: None,
                        evidence: TerminationEvidence::ProcessExit,
                    },
                    RuntimeEvent::Lost {
                        session_id: lost,
                        evidence: LostEvidence::PidNotAlive,
                    },
                ],
                cursor: 42,
            },
        )
        .await
        .or_panic("batch applies");

        assert_eq!(outcome, BatchOutcome::Advanced);
        assert_eq!(cursor, Some(42));
        assert_eq!(session_state(state, running).await, SessionState::Running);
        assert_eq!(
            session_state(state, terminated).await,
            SessionState::Terminated
        );
        assert_eq!(
            session_state(state, lost).await,
            SessionState::Lost {
                evidence: LostEvidence::PidNotAlive
            }
        );
        assert_eq!(stored_cursor(state).await, Some(42));
    }

    #[tokio::test]
    async fn handle_batch_reconciles_status_when_cursor_expires() {
        let test = test_state().await;
        let state = &test.daemon;
        let session_id = insert_session(state, SessionState::Running).await;
        insert_runtime_lifecycle(
            &test,
            Lifecycle {
                session_id,
                runtime: RuntimeKind::Claude,
                isolation: IsolationPolicy::default(),
                state: LifecycleState::Lost(LostEvidence::PidReuseDetected),
                shim_pid: None,
                runtime_pid: Some(101),
                start_time: Some(Utc::now()),
                tmux_pane: None,
                log_availability: None,
            },
        )
        .await;
        let mut cursor = Some(1);

        let outcome = handle_batch(state, &mut cursor, EventBatch::CursorExpired { oldest: 9 })
            .await
            .or_panic("cursor expiry reconciles");

        assert_eq!(outcome, BatchOutcome::Reconciled);
        assert_eq!(cursor, Some(9));
        assert_eq!(
            session_state(state, session_id).await,
            SessionState::Lost {
                evidence: LostEvidence::PidReuseDetected
            }
        );
        assert_eq!(stored_cursor(state).await, Some(9));
    }

    async fn test_state() -> TestState {
        let audit_dir = tempfile::tempdir().or_panic("tempdir creates");
        let identity = IdentityClient::connect(&audit_dir.path().join("audit.sqlite"), 42)
            .await
            .or_panic("identity client connects");
        let dir = tempfile::tempdir().or_panic("store tempdir creates");
        let paths = LiloPaths::new(
            LiloHome::from_path(dir.path().join("lilo")).or_panic("lilo home resolves"),
        );
        let db = LiloDb::open(&paths).await.or_panic("store db opens");
        let store = SqliteStore::open(&db);
        let runtime = Arc::new(
            RuntimeService::build(RuntimeServiceContext::new(
                DaemonConfig::from_lilo_paths(&paths).or_panic("runtime config resolves"),
                db.clone(),
            ))
            .await
            .or_panic("runtime service builds"),
        );
        let runtime_lifecycles = LifecycleStore::open(&db);
        let runtime_port = Arc::new(InProcessRuntime::new(Arc::clone(&runtime)));
        std::mem::forget(dir);
        TestState {
            daemon: DaemonState::new(store, runtime_port, Arc::new(identity), runtime),
            runtime_lifecycles,
        }
    }

    async fn insert_runtime_lifecycle(state: &TestState, lifecycle: Lifecycle) {
        let mut forking = lifecycle.clone();
        forking.state = LifecycleState::Forking;
        state
            .runtime_lifecycles
            .insert_forking(&forking)
            .await
            .or_panic("runtime lifecycle inserts");
        state
            .runtime_lifecycles
            .update_lifecycle(&lifecycle)
            .await
            .or_panic("runtime lifecycle updates");
    }

    async fn insert_session(state: &DaemonState, session_state: SessionState) -> Uuid {
        let session = test_session(session_state);
        let session_id = session.id;
        state
            .store
            .insert_session(&session)
            .await
            .or_panic("session inserts");
        session_id
    }

    async fn session_state(state: &DaemonState, session_id: Uuid) -> SessionState {
        state
            .store
            .get_session(&session_id)
            .await
            .or_panic("session loads")
            .or_panic("session exists")
            .state
    }

    async fn stored_cursor(state: &DaemonState) -> Option<EventCursor> {
        state.store.event_cursor().await.or_panic("cursor loads")
    }

    fn test_session(state: SessionState) -> Session {
        let now = Utc::now();
        Session {
            id: Uuid::now_v7(),
            runtime: SmRuntimeKind::Claude,
            role: "engineer".to_string(),
            workspace: "test".to_string(),
            namespace: Namespace::default(),
            dir: PathBuf::from("test"),
            state,
            runtime_pid: 42,
            runtime_session: None,
            transcript_path: None,
            tmux_pane: None,
            agent_config: None,
            created_at: now,
            started_at: now,
            terminated_at: None,
            exit_code: None,
            updated_at: now,
            labels: Vec::<Label>::new(),
        }
    }
}
