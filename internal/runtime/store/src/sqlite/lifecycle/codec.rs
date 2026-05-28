use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use lilo_rm_core::{
    IsolationPolicy, Lifecycle, LifecycleCounts, LifecycleState, LostEvidence, RecentLostEvent,
    RuntimeExit, RuntimeKind, TmuxAddress,
};
use uuid::Uuid;

const STATE_FORKING: &str = "Forking";
pub(super) const STATE_RUNNING: &str = "Running";
const STATE_EXITED: &str = "Exited";
pub(super) const STATE_LOST: &str = "Lost";

#[derive(sqlx::FromRow)]
pub(super) struct LifecycleRow {
    session_id: String,
    runtime: String,
    isolation: String,
    state: String,
    shim_pid: Option<i64>,
    runtime_pid: Option<i64>,
    start_time: Option<String>,
    tmux_pane: Option<String>,
    exit_code: Option<i64>,
    exit_signal: Option<i64>,
    lost_evidence: Option<String>,
}

#[derive(sqlx::FromRow)]
pub(super) struct StateCountRow {
    pub(super) state: String,
    pub(super) count: i64,
}

#[derive(sqlx::FromRow)]
pub(super) struct RecentLostRow {
    session_id: String,
    lost_evidence: Option<String>,
    updated_at: String,
}

pub(super) struct EncodedLifecycle {
    pub(super) session_id: String,
    pub(super) runtime: String,
    pub(super) isolation: String,
    pub(super) state: &'static str,
    pub(super) shim_pid: Option<i64>,
    pub(super) runtime_pid: Option<i64>,
    pub(super) start_time: Option<String>,
    pub(super) tmux_pane: Option<String>,
    pub(super) exit_code: Option<i64>,
    pub(super) exit_signal: Option<i64>,
    pub(super) lost_evidence: Option<&'static str>,
    pub(super) now: String,
}

type EncodedState = (&'static str, Option<i32>, Option<i32>, Option<&'static str>);

impl EncodedLifecycle {
    pub(super) fn from_lifecycle(lifecycle: &Lifecycle) -> Result<Self> {
        let (state, exit_code, exit_signal, lost_evidence) = encode_state(&lifecycle.state)?;
        Ok(Self {
            session_id: lifecycle.session_id.to_string(),
            runtime: lifecycle.runtime.to_string(),
            isolation: lifecycle.isolation.to_string(),
            state,
            shim_pid: lifecycle.shim_pid.map(i64::from),
            runtime_pid: lifecycle.runtime_pid.map(i64::from),
            start_time: lifecycle.start_time.map(|time| time.to_rfc3339()),
            tmux_pane: encode_tmux_pane(lifecycle.tmux_pane.as_ref())?,
            exit_code: exit_code.map(i64::from),
            exit_signal: exit_signal.map(i64::from),
            lost_evidence,
            now: Utc::now().to_rfc3339(),
        })
    }
}

impl TryFrom<LifecycleRow> for Lifecycle {
    type Error = anyhow::Error;

    fn try_from(row: LifecycleRow) -> Result<Self> {
        Ok(Self {
            session_id: Uuid::parse_str(&row.session_id)?,
            runtime: RuntimeKind::from_str(&row.runtime)?,
            isolation: IsolationPolicy::from_str(&row.isolation)?,
            state: decode_state(&row)?,
            shim_pid: decode_u32(row.shim_pid, "shim_pid")?,
            runtime_pid: decode_u32(row.runtime_pid, "runtime_pid")?,
            start_time: row.start_time.map(|time| parse_time(&time)).transpose()?,
            tmux_pane: decode_tmux_pane(row.tmux_pane)?,
            log_availability: None,
        })
    }
}

impl TryFrom<RecentLostRow> for RecentLostEvent {
    type Error = anyhow::Error;

    fn try_from(row: RecentLostRow) -> Result<Self> {
        Ok(Self {
            session_id: Uuid::parse_str(&row.session_id)?,
            evidence: decode_lost(row.lost_evidence.as_deref())?,
            occurred_at: parse_time(&row.updated_at)?,
        })
    }
}

pub(super) fn encode_tmux_pane(tmux_pane: Option<&TmuxAddress>) -> Result<Option<String>> {
    Ok(tmux_pane.map(serde_json::to_string).transpose()?)
}

fn decode_tmux_pane(tmux_pane: Option<String>) -> Result<Option<TmuxAddress>> {
    tmux_pane
        .map(|value| -> Result<TmuxAddress> {
            if let Ok(pane) = serde_json::from_str::<TmuxAddress>(&value) {
                return Ok(pane);
            }
            Ok(value.parse()?)
        })
        .transpose()
        .context("invalid stored tmux pane")
}

fn encode_state(state: &LifecycleState) -> Result<EncodedState> {
    match state {
        LifecycleState::Forking => Ok((STATE_FORKING, None, None, None)),
        LifecycleState::Running => Ok((STATE_RUNNING, None, None, None)),
        LifecycleState::Exited(exit) => Ok((STATE_EXITED, exit.code, exit.signal, None)),
        LifecycleState::Lost(evidence) => {
            Ok((STATE_LOST, None, None, Some(encode_lost(*evidence)?)))
        }
        _ => Err(anyhow!("unsupported lifecycle state variant: {state:?}")),
    }
}

fn decode_state(row: &LifecycleRow) -> Result<LifecycleState> {
    match row.state.as_str() {
        STATE_FORKING => Ok(LifecycleState::Forking),
        STATE_RUNNING => Ok(LifecycleState::Running),
        STATE_EXITED => Ok(LifecycleState::Exited(RuntimeExit::new(
            decode_i32(row.exit_code, "exit_code")?,
            decode_i32(row.exit_signal, "exit_signal")?,
        ))),
        STATE_LOST => Ok(LifecycleState::Lost(decode_lost(
            row.lost_evidence.as_deref(),
        )?)),
        state => Err(anyhow!("unknown lifecycle state {state}")),
    }
}

pub(super) fn count_lifecycle_state(
    counts: &mut LifecycleCounts,
    state: &str,
    count: u64,
) -> Result<()> {
    match state {
        STATE_FORKING => counts.forking = count,
        STATE_RUNNING => counts.running = count,
        STATE_EXITED => counts.exited = count,
        STATE_LOST => counts.lost = count,
        state => return Err(anyhow!("unknown lifecycle state {state}")),
    }
    Ok(())
}

fn encode_lost(evidence: LostEvidence) -> Result<&'static str> {
    match evidence {
        LostEvidence::ShimDiedBeforeReport => Ok("ShimDiedBeforeReport"),
        LostEvidence::PidNotAlive => Ok("PidNotAlive"),
        LostEvidence::PidReuseDetected => Ok("PidReuseDetected"),
        _ => Err(anyhow!("unsupported lost evidence variant: {evidence:?}")),
    }
}

fn decode_lost(value: Option<&str>) -> Result<LostEvidence> {
    match value {
        Some("ShimDiedBeforeReport") => Ok(LostEvidence::ShimDiedBeforeReport),
        Some("PidNotAlive") => Ok(LostEvidence::PidNotAlive),
        Some("PidReuseDetected") => Ok(LostEvidence::PidReuseDetected),
        Some(other) => Err(anyhow!("unknown lost evidence {other}")),
        None => Err(anyhow!("lost lifecycle missing evidence")),
    }
}

fn decode_u32(value: Option<i64>, field: &'static str) -> Result<Option<u32>> {
    value
        .map(|inner| u32::try_from(inner).with_context(|| format!("{field} out of range")))
        .transpose()
}

fn decode_i32(value: Option<i64>, field: &'static str) -> Result<Option<i32>> {
    value
        .map(|inner| i32::try_from(inner).with_context(|| format!("{field} out of range")))
        .transpose()
}

pub(super) fn parse_time(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}
