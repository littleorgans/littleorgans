use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use chrono::Utc;
use lilo_rm_core::{IsolationPolicy, Lifecycle, LostEvidence, RuntimeEvent};
use lilo_runtime_platform::process::ProcessStartTime;
use tokio::sync::broadcast;
use tokio::time::{Instant, sleep_until};

use crate::{docker_runtime, server::ServerState};

pub const PROBE_SWEEP_INTERVAL: Duration = Duration::from_secs(30);
const RESUME_POLL_INTERVAL: Duration = Duration::from_secs(1);
const RESUME_GAP_THRESHOLD: chrono::Duration = chrono::Duration::seconds(3);

#[derive(Clone, Copy, Debug)]
pub struct ReconcileConfig {
    pub sweep_interval: Duration,
    pub resume_poll_interval: Duration,
    pub resume_gap_threshold: chrono::Duration,
}

impl ReconcileConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            sweep_interval: duration_env("RTM_PROBE_SWEEP_INTERVAL_MS", PROBE_SWEEP_INTERVAL)?,
            resume_poll_interval: duration_env(
                "RTM_RESUME_POLL_INTERVAL_MS",
                RESUME_POLL_INTERVAL,
            )?,
            resume_gap_threshold: chrono_duration_env(
                "RTM_RESUME_GAP_THRESHOLD_MS",
                RESUME_GAP_THRESHOLD,
            )?,
        })
    }
}

impl Default for ReconcileConfig {
    fn default() -> Self {
        Self {
            sweep_interval: PROBE_SWEEP_INTERVAL,
            resume_poll_interval: RESUME_POLL_INTERVAL,
            resume_gap_threshold: RESUME_GAP_THRESHOLD,
        }
    }
}

pub trait ProcessProbe {
    fn pid_alive(&self, pid: u32) -> bool;
    fn start_time_for_pid(&self, pid: u32) -> Result<ProcessStartTime>;
}

trait RuntimeLiveness {
    fn lost_evidence(&self, lifecycle: &Lifecycle) -> Result<Option<LostEvidence>>;
}

pub struct SystemProcessProbe;

struct ReconcileLiveness<'a, P, D> {
    process: &'a P,
    docker: &'a D,
}

struct DockerCliLiveness;

impl ProcessProbe for SystemProcessProbe {
    fn pid_alive(&self, pid: u32) -> bool {
        lilo_runtime_platform::process::pid_alive(pid)
    }

    fn start_time_for_pid(&self, pid: u32) -> Result<ProcessStartTime> {
        lilo_runtime_platform::process::start_time_probe_for_pid(pid)
    }
}

trait DockerLiveness {
    fn lost_evidence(&self, lifecycle: &Lifecycle) -> Result<Option<LostEvidence>>;
}

impl DockerLiveness for DockerCliLiveness {
    fn lost_evidence(&self, lifecycle: &Lifecycle) -> Result<Option<LostEvidence>> {
        let running = docker_runtime::container_running_blocking(lifecycle.session_id)?;
        if running {
            Ok(None)
        } else {
            Ok(Some(LostEvidence::PidNotAlive))
        }
    }
}

impl<P, D> RuntimeLiveness for ReconcileLiveness<'_, P, D>
where
    P: ProcessProbe,
    D: DockerLiveness,
{
    fn lost_evidence(&self, lifecycle: &Lifecycle) -> Result<Option<LostEvidence>> {
        match &lifecycle.isolation {
            IsolationPolicy::Host => host_lost_evidence(lifecycle, self.process),
            IsolationPolicy::Docker(_) => self.docker.lost_evidence(lifecycle),
        }
    }
}

pub async fn reconcile_startup(
    state: Arc<ServerState>,
    probe: &impl ProcessProbe,
) -> Result<Vec<RuntimeEvent>> {
    reconcile_once(
        state,
        &ReconcileLiveness {
            process: probe,
            docker: &DockerCliLiveness,
        },
    )
    .await
}

pub async fn run_periodic<P>(
    state: Arc<ServerState>,
    probe: P,
    shutdown_rx: broadcast::Receiver<()>,
    config: ReconcileConfig,
) where
    P: ProcessProbe + Send + Sync + 'static,
{
    run_periodic_with_config(state, probe, shutdown_rx, config).await;
}

async fn run_periodic_with_config<P>(
    state: Arc<ServerState>,
    probe: P,
    mut shutdown_rx: broadcast::Receiver<()>,
    config: ReconcileConfig,
) where
    P: ProcessProbe + Send + Sync + 'static,
{
    let mut next_deadline = Instant::now() + config.sweep_interval;
    let mut last_wall_tick = Utc::now();

    loop {
        let poll_deadline =
            std::cmp::min(next_deadline, Instant::now() + config.resume_poll_interval);
        tokio::select! {
            _ = shutdown_rx.recv() => break,
            () = sleep_until(poll_deadline) => {
                let now = Instant::now();
                let wall_now = Utc::now();
                let resumed = wall_now - last_wall_tick > config.resume_gap_threshold;
                last_wall_tick = wall_now;

                if now >= next_deadline || resumed {
                    let liveness = ReconcileLiveness {
                        process: &probe,
                        docker: &DockerCliLiveness,
                    };
                    if let Err(error) = reconcile_once(Arc::clone(&state), &liveness).await {
                        tracing::warn!(%error, "periodic reconciliation failed");
                    }
                    next_deadline = if now >= next_deadline {
                        advance_deadline(next_deadline, config.sweep_interval, now)
                    } else {
                        now + config.sweep_interval
                    };
                }
            }
        }
    }
}

fn advance_deadline(mut deadline: Instant, interval: Duration, now: Instant) -> Instant {
    deadline += interval;
    while deadline <= now {
        deadline += interval;
    }
    deadline
}

async fn reconcile_once(
    state: Arc<ServerState>,
    liveness: &impl RuntimeLiveness,
) -> Result<Vec<RuntimeEvent>> {
    let mut events = Vec::new();
    for lifecycle in state.store().running().await? {
        if let Some(evidence) = liveness.lost_evidence(&lifecycle)? {
            if let Some(event) = state.record_lost(lifecycle.session_id, evidence).await? {
                events.push(event);
            }
            continue;
        }
        let runtime_pid = lifecycle
            .runtime_pid
            .ok_or_else(|| anyhow!("running lifecycle missing runtime pid"))?;
        state
            .start_exit_watcher(lifecycle.session_id, runtime_pid)
            .await?;
    }
    state.store().record_probe_sweep(Utc::now()).await?;
    Ok(events)
}

fn host_lost_evidence(
    lifecycle: &Lifecycle,
    probe: &impl ProcessProbe,
) -> Result<Option<LostEvidence>> {
    let runtime_pid = lifecycle
        .runtime_pid
        .ok_or_else(|| anyhow!("running lifecycle missing runtime pid"))?;
    if !probe.pid_alive(runtime_pid) {
        return Ok(Some(LostEvidence::PidNotAlive));
    }

    let Some(stored_start_time) = lifecycle.start_time else {
        return Ok(None);
    };
    let current_start_time = match probe.start_time_for_pid(runtime_pid) {
        Ok(ProcessStartTime::Known(start_time)) => start_time,
        Ok(ProcessStartTime::Gone) => return Ok(Some(LostEvidence::PidNotAlive)),
        Ok(ProcessStartTime::Unsupported) => {
            if !probe.pid_alive(runtime_pid) {
                return Ok(Some(LostEvidence::PidNotAlive));
            }
            return Ok(None);
        }
        Err(error) => {
            if !probe.pid_alive(runtime_pid) {
                return Ok(Some(LostEvidence::PidNotAlive));
            }
            return Err(error);
        }
    };
    if current_start_time != stored_start_time {
        return Ok(Some(LostEvidence::PidReuseDetected));
    }
    Ok(None)
}

fn duration_env(name: &'static str, default: Duration) -> Result<Duration> {
    let Some(value) = std::env::var_os(name) else {
        return Ok(default);
    };
    let millis = value
        .to_string_lossy()
        .parse::<u64>()
        .map_err(|error| anyhow!("{name} must be milliseconds: {error}"))?;
    Ok(Duration::from_millis(millis))
}

fn chrono_duration_env(name: &'static str, default: chrono::Duration) -> Result<chrono::Duration> {
    let Some(value) = std::env::var_os(name) else {
        return Ok(default);
    };
    let millis = value
        .to_string_lossy()
        .parse::<i64>()
        .map_err(|error| anyhow!("{name} must be milliseconds: {error}"))?;
    Ok(chrono::Duration::milliseconds(millis))
}

#[cfg(test)]
mod tests;
