use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use lilo_common::id::SessionId;
use serde::{Deserialize, Serialize};

use super::{RuntimeKind, TmuxAddress};
use crate::IsolationPolicy;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Forking,
    Running,
    Exited(RuntimeExit),
    Lost(LostEvidence),
}

impl Display for LifecycleState {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Forking => formatter.write_str("Forking"),
            Self::Running => formatter.write_str("Running"),
            Self::Exited(exit) => write!(formatter, "Exited({exit})"),
            Self::Lost(evidence) => write!(formatter, "Lost({evidence})"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShimReady {
    pub session_id: SessionId,
    pub shim_pid: u32,
    pub runtime_pid: u32,
    pub start_time: DateTime<Utc>,
    #[serde(default)]
    pub tmux_pane: Option<TmuxAddress>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShimLaunchRequest {
    pub session_id: SessionId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShimExit {
    pub session_id: SessionId,
    pub exit: RuntimeExit,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Lifecycle {
    pub session_id: SessionId,
    pub runtime: RuntimeKind,
    #[serde(default, skip_serializing_if = "IsolationPolicy::is_host")]
    pub isolation: IsolationPolicy,
    pub state: LifecycleState,
    pub shim_pid: Option<u32>,
    pub runtime_pid: Option<u32>,
    pub start_time: Option<DateTime<Utc>>,
    pub tmux_pane: Option<TmuxAddress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_availability: Option<crate::LogAvailability>,
}

impl Lifecycle {
    pub fn forking(session_id: SessionId, runtime: RuntimeKind) -> Self {
        Self {
            session_id,
            runtime,
            isolation: IsolationPolicy::Host,
            state: LifecycleState::Forking,
            shim_pid: None,
            runtime_pid: None,
            start_time: None,
            tmux_pane: None,
            log_availability: None,
        }
    }

    pub fn mark_running(&mut self, ready: ShimReady) -> bool {
        if self.state != LifecycleState::Forking {
            return false;
        }
        self.state = LifecycleState::Running;
        self.shim_pid = Some(ready.shim_pid);
        self.runtime_pid = Some(ready.runtime_pid);
        self.start_time = Some(ready.start_time);
        self.tmux_pane = ready.tmux_pane;
        true
    }

    pub fn mark_exited(&mut self, exit: RuntimeExit) -> bool {
        match self.state {
            LifecycleState::Forking | LifecycleState::Running => {
                self.state = LifecycleState::Exited(exit);
                true
            }
            LifecycleState::Exited(existing) => {
                if existing != exit {
                    self.state = LifecycleState::Exited(exit);
                }
                false
            }
            LifecycleState::Lost(_) => false,
        }
    }

    pub fn mark_lost(&mut self, evidence: LostEvidence) -> bool {
        match self.state {
            LifecycleState::Forking | LifecycleState::Running => {
                self.state = LifecycleState::Lost(evidence);
                true
            }
            LifecycleState::Exited(_) | LifecycleState::Lost(_) => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeExit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

impl RuntimeExit {
    pub const fn new(code: Option<i32>, signal: Option<i32>) -> Self {
        Self { code, signal }
    }
}

impl Display for RuntimeExit {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match (self.code, self.signal) {
            (Some(code), _) => write!(formatter, "code={code}"),
            (None, Some(signal)) => write!(formatter, "signal={signal}"),
            (None, None) => formatter.write_str("unknown"),
        }
    }
}

lilo_common::define_unit_enum! {
    /// Why rtmd believes a runtime process is gone.
    #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
    #[non_exhaustive]
    #[serde(rename_all = "snake_case")]
    pub enum LostEvidence {
        ShimDiedBeforeReport,
        PidNotAlive,
        PidReuseDetected,
        /// The session is lost with no evidence this build can name.
        ///
        /// Durable records decode here when their text belongs to a producer
        /// this build does not know, including rows an older encoder wrote as
        /// the literal `unknown`. The loss itself is still a fact, so the
        /// record stays readable rather than failing the query it appears in.
        Unknown,
    }
}

impl LostEvidence {
    /// Stable text for durable records, such as the
    /// `session_sessions.lost_evidence` column.
    ///
    /// `#[non_exhaustive]` does not apply inside this crate, so this match is
    /// exhaustive: a new variant fails to compile until it is given a text, and
    /// a variant carrying data fails to compile until someone decides
    /// deliberately how it is stored. Nothing here can fail at runtime, which
    /// is what lets callers encode without an error path.
    ///
    /// Stored rows hold these exact bytes, so they outlive the variant names
    /// that produced them: renaming a variant is a storage migration, not a
    /// refactor. They match the serde representation today, which
    /// `lost_evidence_text_matches_serde_representation` holds in place.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ShimDiedBeforeReport => "shim_died_before_report",
            Self::PidNotAlive => "pid_not_alive",
            Self::PidReuseDetected => "pid_reuse_detected",
            Self::Unknown => "unknown",
        }
    }

    /// Read [`Self::as_str`] text back into a variant.
    ///
    /// Total by construction: the search covers [`Self::ALL`], generated from
    /// the declaration above, and anything it does not match is
    /// [`Self::Unknown`]. A durable record therefore always decodes, whichever
    /// build wrote it.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|evidence| evidence.as_str() == text)
            .unwrap_or(Self::Unknown)
    }
}

impl Display for LostEvidence {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShimDiedBeforeReport => formatter.write_str("ShimDiedBeforeReport"),
            Self::PidNotAlive => formatter.write_str("PidNotAlive"),
            Self::PidReuseDetected => formatter.write_str("PidReuseDetected"),
            Self::Unknown => formatter.write_str("Unknown"),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum TerminationEvidence {
    ShimExit,
    ProcessExit,
    Lost(LostEvidence),
}

impl Display for TerminationEvidence {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShimExit => formatter.write_str("ShimExit"),
            Self::ProcessExit => formatter.write_str("ProcessExit"),
            Self::Lost(evidence) => write!(formatter, "Lost({evidence})"),
        }
    }
}

/// Runtime lifecycle observation emitted by rtmd.
///
/// `RuntimeRpc::Events` returns these values in durable append order. `Running`
/// is recorded after shim ready is stored. `Terminated` and `Lost` are recorded
/// when rtmd observes exit or loss evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum RuntimeEvent {
    Running {
        session_id: SessionId,
        runtime_pid: u32,
        start_time: DateTime<Utc>,
    },
    Terminated {
        session_id: SessionId,
        exit_code: Option<i32>,
        signal: Option<i32>,
        evidence: TerminationEvidence,
    },
    Lost {
        session_id: SessionId,
        evidence: LostEvidence,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(session_id: SessionId) -> ShimReady {
        ShimReady {
            session_id,
            shim_pid: 100,
            runtime_pid: 200,
            start_time: Utc::now(),
            tmux_pane: None,
        }
    }

    #[test]
    fn lost_evidence_text_round_trips_for_every_variant() {
        for evidence in LostEvidence::ALL {
            let text = evidence.as_str();

            assert_eq!(
                LostEvidence::from_text(text),
                evidence,
                "{evidence} stores as {text:?}, which does not read back"
            );
        }
    }

    #[test]
    fn lost_evidence_text_matches_serde_representation() {
        for evidence in LostEvidence::ALL {
            assert_eq!(
                serde_json::to_value(evidence).expect("evidence serializes"),
                serde_json::Value::String(evidence.as_str().to_owned()),
                "{evidence} stores and serializes as different text"
            );
        }
    }

    #[test]
    fn unrecognised_lost_evidence_text_reads_back_as_unknown() {
        assert_eq!(
            LostEvidence::from_text("pid_namespace_gone"),
            LostEvidence::Unknown
        );
        assert_eq!(LostEvidence::from_text(""), LostEvidence::Unknown);
    }

    #[test]
    fn lifecycle_transitions_from_forking_to_running_to_exited() {
        let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
        let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);

        assert_eq!(lifecycle.state, LifecycleState::Forking);
        assert!(lifecycle.mark_running(ready(session_id)));
        assert_eq!(lifecycle.state, LifecycleState::Running);
        assert!(lifecycle.mark_exited(RuntimeExit::new(Some(0), None)));
        assert_eq!(
            lifecycle.state,
            LifecycleState::Exited(RuntimeExit::new(Some(0), None))
        );
    }

    #[test]
    fn lifecycle_transitions_from_forking_to_lost() {
        let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
        let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);

        assert!(lifecycle.mark_lost(LostEvidence::ShimDiedBeforeReport));
        assert_eq!(
            lifecycle.state,
            LifecycleState::Lost(LostEvidence::ShimDiedBeforeReport)
        );
    }

    #[test]
    fn lifecycle_transitions_from_running_to_lost() {
        let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
        let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
        assert!(lifecycle.mark_running(ready(session_id)));

        assert!(lifecycle.mark_lost(LostEvidence::ShimDiedBeforeReport));
        assert_eq!(
            lifecycle.state,
            LifecycleState::Lost(LostEvidence::ShimDiedBeforeReport)
        );
    }
}
