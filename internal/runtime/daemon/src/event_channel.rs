use anyhow::{Result, anyhow};
use lilo_rm_core::{Lifecycle, LifecycleState, LostEvidence, RuntimeEvent, TerminationEvidence};

pub(crate) fn running_event(lifecycle: &Lifecycle) -> Result<RuntimeEvent> {
    let runtime_pid = lifecycle
        .runtime_pid
        .ok_or_else(|| anyhow!("running lifecycle missing runtime pid"))?;
    let start_time = lifecycle
        .start_time
        .ok_or_else(|| anyhow!("running lifecycle missing start time"))?;
    Ok(RuntimeEvent::Running {
        session_id: lifecycle.session_id,
        runtime_pid,
        start_time,
    })
}

pub(crate) fn terminated_event(
    lifecycle: &Lifecycle,
    evidence: TerminationEvidence,
) -> RuntimeEvent {
    let (exit_code, signal) = match lifecycle.state {
        LifecycleState::Exited(exit) => (exit.code, exit.signal),
        _ => (None, None),
    };
    RuntimeEvent::Terminated {
        session_id: lifecycle.session_id,
        exit_code,
        signal,
        evidence,
    }
}

pub(crate) fn lost_event(lifecycle: &Lifecycle, evidence: LostEvidence) -> RuntimeEvent {
    RuntimeEvent::Lost {
        session_id: lifecycle.session_id,
        evidence,
    }
}
