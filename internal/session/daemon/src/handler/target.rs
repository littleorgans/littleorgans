use anyhow::{Context, Result};
use lilo_session_core::{Selector, Session, TargetError};
use uuid::Uuid;

use super::DaemonState;

impl DaemonState {
    pub(crate) fn resolve_selector(
        &self,
        selector: &Selector,
        label: &str,
    ) -> Result<Vec<Session>> {
        let sessions = self
            .store()?
            .list_sessions_by_selector(selector)
            .context("failed to resolve selector")?;
        if !sessions.is_empty() {
            return Ok(sessions);
        }
        match selector {
            Selector::Id { id } if label == "session" => anyhow::bail!("unknown session: {id}"),
            Selector::Id { id } => anyhow::bail!("unknown {label} session: {id}"),
            _ if label == "session" => anyhow::bail!("selector matched no sessions: {selector}"),
            _ => anyhow::bail!("{label} selector matched no sessions: {selector}"),
        }
    }

    pub(super) fn require_session(&self, id: &Uuid, label: &str) -> Result<()> {
        let exists = self
            .store()?
            .get_session(id)
            .context("failed to load session")?
            .is_some();
        anyhow::ensure!(exists, "unknown {label} session: {id}");
        Ok(())
    }
}

pub(super) fn target_error(id: &Uuid, error: &anyhow::Error) -> TargetError {
    TargetError {
        target: id.to_string(),
        message: format!("{error:#}"),
    }
}
