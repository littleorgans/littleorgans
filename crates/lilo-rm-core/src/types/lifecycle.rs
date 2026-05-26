use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    pub session_id: Uuid,
    pub shim_pid: u32,
    pub runtime_pid: u32,
    pub start_time: DateTime<Utc>,
    #[serde(default)]
    pub tmux_pane: Option<TmuxAddress>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShimLaunchRequest {
    pub session_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShimExit {
    pub session_id: Uuid,
    pub exit: RuntimeExit,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Lifecycle {
    pub session_id: Uuid,
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
    pub fn forking(session_id: Uuid, runtime: RuntimeKind) -> Self {
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum LostEvidence {
    ShimDiedBeforeReport,
    PidNotAlive,
    PidReuseDetected,
}

impl Display for LostEvidence {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShimDiedBeforeReport => formatter.write_str("ShimDiedBeforeReport"),
            Self::PidNotAlive => formatter.write_str("PidNotAlive"),
            Self::PidReuseDetected => formatter.write_str("PidReuseDetected"),
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
        session_id: Uuid,
        runtime_pid: u32,
        start_time: DateTime<Utc>,
    },
    Terminated {
        session_id: Uuid,
        exit_code: Option<i32>,
        signal: Option<i32>,
        evidence: TerminationEvidence,
    },
    Lost {
        session_id: Uuid,
        evidence: LostEvidence,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(session_id: Uuid) -> ShimReady {
        ShimReady {
            session_id,
            shim_pid: 100,
            runtime_pid: 200,
            start_time: Utc::now(),
            tmux_pane: None,
        }
    }

    #[test]
    fn lifecycle_transitions_from_forking_to_running_to_exited() {
        let session_id = Uuid::now_v7();
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
        let session_id = Uuid::now_v7();
        let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);

        assert!(lifecycle.mark_lost(LostEvidence::ShimDiedBeforeReport));
        assert_eq!(
            lifecycle.state,
            LifecycleState::Lost(LostEvidence::ShimDiedBeforeReport)
        );
    }

    #[test]
    fn lifecycle_transitions_from_running_to_lost() {
        let session_id = Uuid::now_v7();
        let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
        assert!(lifecycle.mark_running(ready(session_id)));

        assert!(lifecycle.mark_lost(LostEvidence::ShimDiedBeforeReport));
        assert_eq!(
            lifecycle.state,
            LifecycleState::Lost(LostEvidence::ShimDiedBeforeReport)
        );
    }
}
