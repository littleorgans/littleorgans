use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Result, bail};
use lilo_rm_core::{RuntimeExit, TerminationEvidence};
use lilo_runtime_platform::process_exit::{ProcessExitWatcher, watch_process_exit};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::ServerState;

pub(super) struct WatcherCoordinator {
    exit_watchers: Mutex<HashMap<Uuid, ProcessExitWatcher>>,
}

impl WatcherCoordinator {
    pub(super) fn new() -> Self {
        Self {
            exit_watchers: Mutex::new(HashMap::new()),
        }
    }

    pub(super) async fn start_exit_watcher(
        &self,
        state: &Arc<ServerState>,
        session_id: Uuid,
        runtime_pid: u32,
    ) -> Result<()> {
        let mut exit_watchers = self.exit_watchers.lock().await;
        if exit_watchers.contains_key(&session_id) {
            return Ok(());
        }
        let (watcher, exit_rx) = watch_process_exit(runtime_pid)?;
        exit_watchers.insert(session_id, watcher);
        drop(exit_watchers);

        let state = Arc::clone(state);
        tokio::spawn(async move {
            if exit_rx.await.is_ok()
                && let Err(error) = record_watcher_exit(state, session_id).await
            {
                tracing::warn!(%error, %session_id, "process exit watcher failed");
            }
        });
        Ok(())
    }

    pub(super) async fn remove_watcher(&self, session_id: Uuid) {
        self.exit_watchers.lock().await.remove(&session_id);
    }

    pub(super) async fn process_exit_watcher_count(&self) -> usize {
        self.exit_watchers.lock().await.len()
    }
}

async fn record_watcher_exit(state: Arc<ServerState>, session_id: Uuid) -> Result<()> {
    tokio::time::sleep(Duration::from_millis(300)).await;
    if state.is_terminal(session_id).await {
        return Ok(());
    }

    let evidence = state.watcher_evidence(session_id).await?;
    match evidence {
        TerminationEvidence::Lost(lost) => {
            let _ = state.record_lost(session_id, lost).await?;
        }
        TerminationEvidence::ProcessExit => {
            let _ = state
                .record_exited(session_id, RuntimeExit::new(None, None), evidence)
                .await?;
        }
        TerminationEvidence::ShimExit => {}
        _ => bail!(
            "process exit watcher for session {session_id} received unsupported termination evidence variant: {evidence:?}"
        ),
    }
    Ok(())
}
