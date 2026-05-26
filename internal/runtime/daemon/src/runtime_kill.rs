use std::time::{Duration, Instant};

use anyhow::Result;
use lilo_rm_core::{IsolationPolicy, KillOutcome, KillRequest, RuntimeSignal};
use uuid::Uuid;

use crate::{docker_runtime, error::RuntimeFailure, server::ServerState};

const KILL_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(crate) async fn kill_runtime(state: &ServerState, request: KillRequest) -> Result<KillOutcome> {
    let lifecycle = state
        .store()
        .get(request.session_id)
        .await?
        .ok_or_else(|| RuntimeFailure::session_not_found(request.session_id))?;
    match lifecycle.isolation {
        IsolationPolicy::Host => {
            let runtime_pid = lifecycle
                .runtime_pid
                .ok_or_else(|| RuntimeFailure::session_not_found(request.session_id))?;
            let target = HostKillTarget { runtime_pid };
            let mut terminal = StateTerminalCheck::new(state, request.session_id);
            run_kill_loop(&target, &mut terminal, request.signal, request.grace_secs).await
        }
        IsolationPolicy::Docker(_) => {
            let target = DockerKillTarget {
                session_id: request.session_id,
            };
            let mut terminal = StateTerminalCheck::new(state, request.session_id);
            run_kill_loop(&target, &mut terminal, request.signal, request.grace_secs).await
        }
    }
}

async fn run_kill_loop<T, C>(
    target: &T,
    terminal: &mut C,
    signal: RuntimeSignal,
    grace_secs: u64,
) -> Result<KillOutcome>
where
    T: KillTarget,
    C: TerminalCheck,
{
    let outcome = target.send_signal(signal).await?;
    if matches!(outcome, KillOutcome::AlreadyExited) {
        return Ok(outcome);
    }
    let deadline = Instant::now() + Duration::from_secs(grace_secs);

    while Instant::now() < deadline {
        if terminal.is_terminal().await? || !target.is_alive().await? {
            return Ok(outcome);
        }
        tokio::time::sleep(KILL_POLL_INTERVAL).await;
    }

    if target.is_alive().await? && signal != RuntimeSignal::Kill {
        target.send_kill().await?;
    }
    Ok(outcome)
}

trait KillTarget {
    async fn send_signal(&self, signal: RuntimeSignal) -> Result<KillOutcome>;

    async fn send_kill(&self) -> Result<()>;

    async fn is_alive(&self) -> Result<bool>;
}

trait TerminalCheck {
    async fn is_terminal(&mut self) -> Result<bool>;
}

struct HostKillTarget {
    runtime_pid: u32,
}

impl KillTarget for HostKillTarget {
    async fn send_signal(&self, signal: RuntimeSignal) -> Result<KillOutcome> {
        lilo_runtime_platform::signal::send_signal_for_kill(self.runtime_pid, signal)
    }

    async fn send_kill(&self) -> Result<()> {
        lilo_runtime_platform::signal::send_signal(self.runtime_pid, RuntimeSignal::Kill)
    }

    async fn is_alive(&self) -> Result<bool> {
        Ok(lilo_runtime_platform::process::pid_alive(self.runtime_pid))
    }
}

struct DockerKillTarget {
    session_id: Uuid,
}

impl KillTarget for DockerKillTarget {
    async fn send_signal(&self, signal: RuntimeSignal) -> Result<KillOutcome> {
        docker_runtime::kill_container(self.session_id, signal).await
    }

    async fn send_kill(&self) -> Result<()> {
        docker_runtime::kill_container(self.session_id, RuntimeSignal::Kill)
            .await
            .map(|_| ())
    }

    async fn is_alive(&self) -> Result<bool> {
        docker_runtime::DockerCliRuntime
            .running(self.session_id)
            .await
    }
}

struct StateTerminalCheck<'a> {
    state: &'a ServerState,
    session_id: Uuid,
}

impl<'a> StateTerminalCheck<'a> {
    fn new(state: &'a ServerState, session_id: Uuid) -> Self {
        Self { state, session_id }
    }
}

impl TerminalCheck for StateTerminalCheck<'_> {
    async fn is_terminal(&mut self) -> Result<bool> {
        Ok(self.state.is_terminal(self.session_id).await)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::test]
    async fn shared_kill_loop_escalates_alive_target_after_grace_deadline() {
        let target = FakeKillTarget::new();
        let mut terminal = FakeTerminalCheck;

        let outcome = run_kill_loop(&target, &mut terminal, RuntimeSignal::Term, 0)
            .await
            .expect("kill loop");

        assert_eq!(outcome, KillOutcome::Signalled);
        assert_eq!(
            target.signals(),
            vec![RuntimeSignal::Term, RuntimeSignal::Kill]
        );
    }

    struct FakeKillTarget {
        signals: Arc<Mutex<Vec<RuntimeSignal>>>,
    }

    impl FakeKillTarget {
        fn new() -> Self {
            Self {
                signals: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn signals(&self) -> Vec<RuntimeSignal> {
            self.signals.lock().expect("signals").clone()
        }
    }

    impl KillTarget for FakeKillTarget {
        async fn send_signal(&self, signal: RuntimeSignal) -> Result<KillOutcome> {
            self.signals.lock().expect("signals").push(signal);
            Ok(KillOutcome::Signalled)
        }

        async fn send_kill(&self) -> Result<()> {
            self.signals
                .lock()
                .expect("signals")
                .push(RuntimeSignal::Kill);
            Ok(())
        }

        async fn is_alive(&self) -> Result<bool> {
            Ok(true)
        }
    }

    struct FakeTerminalCheck;

    impl TerminalCheck for FakeTerminalCheck {
        async fn is_terminal(&mut self) -> Result<bool> {
            Ok(false)
        }
    }
}
