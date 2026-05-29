use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use lilo_session_core::Session;
use lilo_session_driver::ChildExit;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::handler::DaemonState;

pub struct LifecycleTask {
    handle: JoinHandle<()>,
}

impl LifecycleTask {
    pub fn spawn(state: Arc<DaemonState>) -> Self {
        let handle = tokio::spawn(async move {
            loop {
                if let Err(error) = refresh_exits(&state).await {
                    tracing::warn!(error = ?error, "failed to refresh session lifecycle");
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });

        Self { handle }
    }
}

impl Drop for LifecycleTask {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

pub async fn refresh_exits(state: &DaemonState) -> Result<()> {
    for child_exit in state
        .runtime
        .reap_exited()
        .await
        .context("failed to reap children")?
    {
        persist_child_exit(state, child_exit)
            .await
            .context("failed to persist terminated session")?;
    }
    Ok(())
}

pub async fn persist_child_exit(
    state: &DaemonState,
    child_exit: ChildExit,
) -> Result<Option<Session>> {
    let id = Uuid::parse_str(&child_exit.session_id).context("invalid session id")?;
    let now = Utc::now();
    let store = state.store();
    if let Some(transcript_path) = child_exit.transcript_path {
        store
            .record_transcript_path(&id, &transcript_path, now)
            .await
            .context("failed to persist transcript path")?;
    }
    store
        .mark_session_terminated(&id, child_exit.exit_code, now)
        .await
        .map_err(Into::into)
}
