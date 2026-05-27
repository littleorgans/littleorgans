use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use lilo_rm_client::{EventWatcher, RuntimeClient};
use lilo_rm_core::{EventBatch, EventCursor, StatusFilter};
use tokio::task::JoinHandle;

use crate::handler::DaemonState;

const EVENT_WAIT_MS: u32 = 30_000;
const BACKOFF_INITIAL: Duration = Duration::from_millis(200);
const BACKOFF_MAX: Duration = Duration::from_secs(5);

pub struct RuntimeEventTask {
    handle: JoinHandle<()>,
}

impl RuntimeEventTask {
    pub fn spawn(state: Arc<DaemonState>, socket_path: PathBuf) -> Self {
        let handle = tokio::spawn(async move {
            if let Err(error) = run_event_loop(state, socket_path).await {
                eprintln!("runtime event loop stopped: {error:#}");
            }
        });

        Self { handle }
    }
}

impl Drop for RuntimeEventTask {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn run_event_loop(state: Arc<DaemonState>, socket_path: PathBuf) -> Result<()> {
    let mut cursor = state
        .store()
        .event_cursor()
        .await
        .context("failed to load runtime event cursor")?;
    let mut backoff = BACKOFF_INITIAL;

    loop {
        let client = RuntimeClient::new(socket_path.clone());
        let status_client = client.clone();
        let mut builder = EventWatcher::builder().wait_ms(EVENT_WAIT_MS);
        if let Some(cursor) = cursor {
            builder = builder.since(cursor);
        }
        let mut watcher = match builder.connect(client).await {
            Ok(watcher) => watcher,
            Err(error) => {
                eprintln!("failed to connect runtime event watcher: {error:#}");
                tokio::time::sleep(backoff).await;
                backoff = next_backoff(backoff);
                continue;
            }
        };
        backoff = BACKOFF_INITIAL;

        loop {
            match watcher.next().await {
                Ok(batch) => match handle_batch(&state, &status_client, &mut cursor, batch).await {
                    Ok(_) => {
                        backoff = BACKOFF_INITIAL;
                    }
                    Err(error) => {
                        eprintln!("unsupported runtime event batch: {error:#}");
                        tokio::time::sleep(backoff).await;
                        backoff = next_backoff(backoff);
                        break;
                    }
                },
                Err(error) => {
                    eprintln!("runtime event watcher failed: {error:#}");
                    tokio::time::sleep(backoff).await;
                    backoff = next_backoff(backoff);
                    break;
                }
            }
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
    status_client: &RuntimeClient,
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
            let payload = status_client
                .status(StatusFilter::empty())
                .await
                .context("failed to reconcile expired runtime cursor")?;
            crate::reconcile::reconcile_lifecycles(state, &payload.lifecycles).await?;
            state
                .store()
                .apply_cursor(oldest)
                .await
                .context("failed to persist expired runtime cursor")?;
            *cursor = Some(oldest);
            Ok(BatchOutcome::Reconciled)
        }
        batch => bail!("unsupported runtime event batch: {batch:?}"),
    }
}

fn next_backoff(current: Duration) -> Duration {
    std::cmp::min(current.saturating_mul(2), BACKOFF_MAX)
}

#[cfg(test)]
mod tests {
    use crate::test_support::OrPanic as _;
    use std::path::Path;

    use chrono::Utc;
    use lilo_db::LiloDb;
    use lilo_paths::{LiloHome, LiloPaths};
    use lilo_rm_core::{
        IsolationPolicy, Lifecycle, LifecycleState, LostEvidence, RuntimeEvent, RuntimeKind,
        RuntimeResponse, StatusPayload, TerminationEvidence, read_json_line, write_json_line,
    };
    use lilo_runtime_daemon::{DaemonConfig, RuntimeService, RuntimeServiceContext};
    use lilo_session_core::{
        Label, Namespace, RuntimeKind as SmRuntimeKind, Session, SessionState,
    };
    use lilo_session_driver::RtmdDriver;
    use lilo_session_store::SqliteStore;
    use lilo_wire::LilodRpc;
    use tokio::io::BufReader;
    use tokio::net::UnixListener;
    use uuid::Uuid;

    use crate::identity_client::IdentityClient;

    use super::*;

    #[tokio::test]
    async fn handle_batch_applies_events_and_advances_cursor() {
        let state = test_state().await;
        let running = insert_session(&state, SessionState::Spawning).await;
        let terminated = insert_session(&state, SessionState::Running).await;
        let lost = insert_session(&state, SessionState::Running).await;
        let mut cursor = None;

        let outcome = handle_batch(
            &state,
            &RuntimeClient::new("/unused.sock"),
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
        assert_eq!(session_state(&state, running).await, SessionState::Running);
        assert_eq!(
            session_state(&state, terminated).await,
            SessionState::Terminated
        );
        assert_eq!(
            session_state(&state, lost).await,
            SessionState::Lost {
                evidence: LostEvidence::PidNotAlive
            }
        );
        assert_eq!(stored_cursor(&state).await, Some(42));
    }

    #[tokio::test]
    async fn handle_batch_reconciles_status_when_cursor_expires() {
        let state = test_state().await;
        let session_id = insert_session(&state, SessionState::Running).await;
        let socket_dir = tempfile::tempdir().or_panic("socket dir creates");
        let socket_path = socket_dir.path().join("rtmd.sock");
        let server = spawn_status_server(
            &socket_path,
            vec![Lifecycle {
                session_id,
                runtime: RuntimeKind::Claude,
                isolation: IsolationPolicy::default(),
                state: LifecycleState::Lost(LostEvidence::PidReuseDetected),
                shim_pid: None,
                runtime_pid: Some(101),
                start_time: Some(Utc::now()),
                tmux_pane: None,
                log_availability: None,
            }],
        );
        let mut cursor = Some(1);

        let outcome = handle_batch(
            &state,
            &RuntimeClient::new(socket_path),
            &mut cursor,
            EventBatch::CursorExpired { oldest: 9 },
        )
        .await
        .or_panic("cursor expiry reconciles");
        server.await.or_panic("status server completes");

        assert_eq!(outcome, BatchOutcome::Reconciled);
        assert_eq!(cursor, Some(9));
        assert_eq!(
            session_state(&state, session_id).await,
            SessionState::Lost {
                evidence: LostEvidence::PidReuseDetected
            }
        );
        assert_eq!(stored_cursor(&state).await, Some(9));
    }

    async fn test_state() -> DaemonState {
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
                db,
            ))
            .await
            .or_panic("runtime service builds"),
        );
        std::mem::forget(dir);
        DaemonState::new(
            store,
            Arc::new(RtmdDriver::new(paths.socket_path())),
            Arc::new(identity),
            runtime,
        )
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

    fn spawn_status_server(
        socket_path: &Path,
        lifecycles: Vec<Lifecycle>,
    ) -> tokio::task::JoinHandle<()> {
        let listener = UnixListener::bind(socket_path).or_panic("listener binds");
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.or_panic("client connects");
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let envelope: LilodRpc = read_json_line(&mut reader)
                .await
                .or_panic("status request reads");
            let LilodRpc::Runtime(_) = envelope else {
                panic!("expected runtime rpc");
            };
            write_json_line(
                &mut writer,
                &RuntimeResponse::Status(StatusPayload { lifecycles }),
            )
            .await
            .or_panic("status response writes");
        })
    }
}
