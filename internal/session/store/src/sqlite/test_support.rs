use std::path::PathBuf;

use chrono::Utc;
use lilo_session_core::{Label, Namespace, RuntimeKind, Session, SessionState};
use uuid::Uuid;

pub(crate) fn running_session(role: &str, workspace: &str) -> Session {
    let now = Utc::now();
    Session {
        id: Uuid::now_v7(),
        runtime: RuntimeKind::Claude,
        role: role.to_string(),
        workspace: workspace.to_string(),
        namespace: Namespace::default(),
        dir: PathBuf::from(workspace),
        state: SessionState::Running,
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
