use std::time::{Duration, Instant};

use anyhow::Result;
use lilo_common::id::SessionId;
use lilo_rm_core::{IsolationPolicy, KillOutcome, KillRequest, LostEvidence, RuntimeSignal};

use crate::{
    docker_runtime,
    error::RuntimeFailure,
    reconcile::{ProcessProbe, SystemProcessProbe, host_process_lost_evidence},
    server::ServerState,
};

const KILL_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(crate) async fn kill_runtime(state: &ServerState, request: KillRequest) -> Result<KillOutcome> {
    kill_runtime_with_probe(state, request, &SystemProcessProbe).await
}

async fn kill_runtime_with_probe(
    state: &ServerState,
    request: KillRequest,
    probe: &impl ProcessProbe,
) -> Result<KillOutcome> {
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
            if matches!(
                host_process_lost_evidence(runtime_pid, lifecycle.start_time, probe)?,
                Some(LostEvidence::PidReuseDetected)
            ) {
                let _ = state
                    .record_lost(request.session_id, LostEvidence::PidReuseDetected)
                    .await?;
                return Ok(KillOutcome::AlreadyExited);
            }
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
        crate::signal::send_signal_for_kill(self.runtime_pid, signal)
    }

    async fn send_kill(&self) -> Result<()> {
        crate::signal::send_signal(self.runtime_pid, RuntimeSignal::Kill)
    }

    async fn is_alive(&self) -> Result<bool> {
        Ok(lilo_sys::process::pid_alive(self.runtime_pid))
    }
}

struct DockerKillTarget {
    session_id: SessionId,
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
    session_id: SessionId,
}

impl<'a> StateTerminalCheck<'a> {
    fn new(state: &'a ServerState, session_id: SessionId) -> Self {
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

    use chrono::{DateTime, TimeZone, Utc};
    use lilo_rm_core::{Lifecycle, LifecycleState, LostEvidence, RuntimeKind, ShimReady};
    use lilo_runtime_store::LifecycleStore;

    use super::*;
    use crate::{
        reconcile::ReconcileConfig,
        test_support::{ChildGuard, FakeProcessProbe, RuntimeServiceFixture},
    };

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
    async fn kill_runtime_rejects_a_reused_host_pid() {
        let mut child = ChildGuard::spawn();
        let stored_start_time = Utc.timestamp_opt(1_000, 0).unwrap();
        let (fixture, store, state, session_id) =
            running_host_fixture(&child, stored_start_time).await;

        let outcome = kill_runtime_with_probe(
            &state,
            KillRequest {
                session_id,
                signal: RuntimeSignal::Term,
                grace_secs: 0,
            },
            &FakeProcessProbe::new(
                [child.id()],
                [(child.id(), Utc.timestamp_opt(2_000, 0).unwrap())],
            ),
        )
        .await
        .expect("kill outcome");
        let child_alive = child.is_alive();
        let stored_state = store
            .get(session_id)
            .await
            .expect("get lifecycle")
            .expect("lifecycle")
            .state;

        drop(state);
        drop(store);
        fixture.cleanup().await;

        assert_eq!(outcome, KillOutcome::AlreadyExited);
        assert!(child_alive, "kill_runtime signalled a reused pid");
        assert_eq!(
            stored_state,
            LifecycleState::Lost(LostEvidence::PidReuseDetected)
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
    async fn kill_runtime_signals_a_pid_reported_dead_without_recording_lost() {
        let mut child = ChildGuard::spawn();
        let (fixture, store, state, session_id) =
            running_host_fixture(&child, child.start_time()).await;

        let outcome = kill_runtime_with_probe(
            &state,
            KillRequest {
                session_id,
                signal: RuntimeSignal::Term,
                grace_secs: 0,
            },
            &FakeProcessProbe::new([], []),
        )
        .await
        .expect("kill outcome");
        let stored_state = store
            .get(session_id)
            .await
            .expect("get lifecycle")
            .expect("lifecycle")
            .state;

        drop(state);
        drop(store);
        fixture.cleanup().await;

        assert_eq!(outcome, KillOutcome::Signalled);
        child.wait_for_exit();
        assert_eq!(stored_state, LifecycleState::Running);
    }

    async fn running_host_fixture(
        child: &ChildGuard,
        stored_start_time: DateTime<Utc>,
    ) -> (
        RuntimeServiceFixture,
        LifecycleStore,
        ServerState,
        SessionId,
    ) {
        let fixture = RuntimeServiceFixture::new(ReconcileConfig::default()).await;
        let store = LifecycleStore::from_db(fixture.testdb.db());
        let state = ServerState::new(fixture.testdb.db(), fixture.config.clone(), store.clone())
            .expect("state");
        let session_id = SessionId::new();
        let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
        store.insert_forking(&lifecycle).await.expect("insert");
        lifecycle.mark_running(ShimReady {
            session_id,
            shim_pid: child.id() + 1,
            runtime_pid: child.id(),
            start_time: stored_start_time,
            tmux_pane: None,
        });
        store.update_lifecycle(&lifecycle).await.expect("running");
        (fixture, store, state, session_id)
    }

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
