use anyhow::{Context, Result};
use chrono::Utc;
use lilo_rm_core::{Lifecycle, LifecycleState, LogAvailability, LostEvidence, StatusFilter};

use crate::handler::DaemonState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileFinding {
    pub session_id: String,
    pub evidence: String,
}

pub async fn reconcile_once(state: &DaemonState) -> Result<Vec<ReconcileFinding>> {
    let socket_path = state
        .rtmd_socket_path
        .as_ref()
        .context("rtmd socket path is not configured")?;
    let payload = lilo_rm_client::RuntimeClient::new(socket_path.clone())
        .status(StatusFilter::empty())
        .await
        .context("failed to load rtmd lifecycle status")?;
    reconcile_lifecycles(state, &payload.lifecycles).await
}

pub async fn reconcile_lifecycles(
    state: &DaemonState,
    lifecycles: &[Lifecycle],
) -> Result<Vec<ReconcileFinding>> {
    let mut findings = Vec::new();
    for lifecycle in lifecycles {
        if let Some(path) = lifecycle_transcript_path(lifecycle) {
            state
                .store()
                .record_transcript_path(&lifecycle.session_id, path, Utc::now())
                .await
                .context("failed to persist transcript path")?;
        }
        match lifecycle.state {
            LifecycleState::Forking | LifecycleState::Running => {}
            LifecycleState::Exited(exit) => {
                state
                    .store()
                    .mark_session_terminated(&lifecycle.session_id, exit.code, Utc::now())
                    .await
                    .context("failed to mark session terminated")?;
            }
            LifecycleState::Lost(evidence) => {
                mark_lost(state, lifecycle, evidence).await?;
                findings.push(ReconcileFinding {
                    session_id: lifecycle.session_id.to_string(),
                    evidence: format!("rtmd lifecycle lost: {evidence}"),
                });
            }
            _ => {
                findings.push(ReconcileFinding {
                    session_id: lifecycle.session_id.to_string(),
                    evidence: format!("unsupported rtmd lifecycle state: {:?}", lifecycle.state),
                });
            }
        }
    }
    Ok(findings)
}

async fn mark_lost(
    state: &DaemonState,
    lifecycle: &Lifecycle,
    evidence: LostEvidence,
) -> Result<()> {
    state
        .store()
        .mark_session_lost(&lifecycle.session_id, evidence, Utc::now())
        .await
        .context("failed to mark session lost")?;
    Ok(())
}

fn lifecycle_transcript_path(lifecycle: &Lifecycle) -> Option<&std::path::Path> {
    match lifecycle.log_availability.as_ref() {
        Some(LogAvailability::Headless { stdout_path, .. }) => Some(stdout_path.as_path()),
        Some(LogAvailability::TmuxPaneSnapshot | LogAvailability::Unavailable { .. }) | None => {
            None
        }
    }
}
