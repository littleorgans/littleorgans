use std::path::PathBuf;

use lilo_common::id::SessionId;
use lilo_db::test_support::now_micros;
use lilo_rm_core::LostEvidence;
use lilo_session_core::{Label, Namespace, RuntimeKind, Session, SessionState};

/// Every `LostEvidence` variant the runtime can report.
///
/// The enum is `#[non_exhaustive]`, so this list cannot be derived from it
/// outside `lilo-rm-core`. A new variant belongs here, and the lost-evidence
/// round-trip tests then cover it.
pub(crate) const LOST_EVIDENCE_VARIANTS: [LostEvidence; 3] = [
    LostEvidence::ShimDiedBeforeReport,
    LostEvidence::PidNotAlive,
    LostEvidence::PidReuseDetected,
];

pub(crate) fn running_session(role: &str, workspace: &str) -> Session {
    let now = now_micros();
    Session {
        id: SessionId::from_uuid(uuid::Uuid::now_v7()),
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
