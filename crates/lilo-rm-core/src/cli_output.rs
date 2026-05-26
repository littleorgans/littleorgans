use std::fmt::{self, Write};
use std::path::Path;

use serde::Serialize;
use uuid::Uuid;

use crate::{
    DoctorResponse, EventsPayload, KillByPidResponse, KillOutcome, KilledPayload, Lifecycle,
    LifecycleCounts, LogAvailability, NudgeOutcome, NudgeResponse, PaneSnapshot, RuntimeCapability,
    RuntimeEvent, RuntimeResponse, VersionInfo,
};

pub trait CliOutput: Serialize {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Ack {
    pub session_id: Uuid,
}

impl CliOutput for Ack {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        writeln!(f, "kill OK; session_id={}", self.session_id)
    }
}

impl CliOutput for VersionInfo {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        writeln!(
            f,
            "version={} git_sha={} protocol={}",
            self.version, self.git_sha, self.protocol_version
        )
    }
}

impl CliOutput for DoctorResponse {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        writeln!(f, "rtmd")?;
        writeln!(
            f,
            "  version             {} (git: {})",
            self.version.version, self.version.git_sha
        )?;
        writeln!(f, "  protocol            {}", self.version.protocol_version)?;
        writeln!(
            f,
            "  capabilities        {}",
            format_capabilities(&self.version.capabilities)
        )?;
        writeln!(f, "  socket              {}", self.socket_path)?;
        writeln!(
            f,
            "  uptime              {}",
            format_duration(self.uptime_secs)
        )?;
        writeln!(f, "sqlite")?;
        writeln!(
            f,
            "  applied migrations  {} of {} ({})",
            self.sqlite.applied,
            self.sqlite.total,
            format_migrations(&self.sqlite.applied_descriptions)
        )?;
        if !self.sqlite.pending_descriptions.is_empty() {
            writeln!(
                f,
                "  pending migrations  {}",
                format_migrations(&self.sqlite.pending_descriptions)
            )?;
        }
        print_lifecycle_counts(f, &self.lifecycles)?;
        writeln!(
            f,
            "process exit watchers {}",
            self.watchers.process_exit_watchers
        )?;
        writeln!(f, "shim sockets          {}", self.watchers.shim_sockets)?;
        writeln!(f, "event waiters         {}", self.watchers.event_waiters)?;
        writeln!(f, "launchers")?;
        for launcher in &self.launchers {
            let value = launcher
                .command
                .as_deref()
                .or(launcher.error.as_deref())
                .unwrap_or("unavailable");
            writeln!(f, "  {:<18} {}", launcher.runtime, value)?;
        }
        writeln!(f, "tmux                  {}", format_tmux(self))?;
        writeln!(f, "docker")?;
        writeln!(
            f,
            "  cli                 {}",
            format_readiness(&self.docker.cli)
        )?;
        writeln!(
            f,
            "  daemon              {}",
            format_readiness(&self.docker.daemon)
        )?;
        writeln!(
            f,
            "  manifest validation {}",
            format_readiness(&self.docker.manifest_validation)
        )?;
        writeln!(
            f,
            "  isolation           supported={} workspace={} experimental={}",
            self.docker.isolation.supported,
            self.docker.isolation.default_workspace,
            self.docker.isolation.experimental
        )?;
        writeln!(
            f,
            "last probe sweep      {}",
            self.last_probe_sweep
                .map_or_else(|| "never".to_owned(), |time| time.to_rfc3339())
        )?;
        print_recent_lost(f, self)
    }
}

impl CliOutput for Vec<Lifecycle> {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        if self.is_empty() {
            return writeln!(f, "no lifecycles");
        }
        for lifecycle in self {
            writeln!(
                f,
                "session_id={} state={} runtime={} shim_pid={} runtime_pid={} start_time={} tmux_pane={} log_availability={}",
                lifecycle.session_id,
                lifecycle.state,
                lifecycle.runtime,
                display_optional_u32(lifecycle.shim_pid),
                display_optional_u32(lifecycle.runtime_pid),
                lifecycle
                    .start_time
                    .map_or_else(|| "-".to_owned(), |time| time.to_rfc3339()),
                lifecycle
                    .tmux_pane
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), ToString::to_string),
                format_log_availability(lifecycle.log_availability.as_ref())
            )?;
        }
        Ok(())
    }
}

impl CliOutput for Vec<RuntimeEvent> {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        for event in self {
            match event {
                RuntimeEvent::Running {
                    session_id,
                    runtime_pid,
                    start_time,
                } => writeln!(
                    f,
                    "runtime event=Running session_id={} runtime_pid={} start_time={}",
                    session_id,
                    runtime_pid,
                    start_time.to_rfc3339()
                )?,
                RuntimeEvent::Terminated {
                    session_id,
                    exit_code,
                    signal,
                    evidence,
                } => writeln!(
                    f,
                    "runtime event=Terminated session_id={} exit_code={} signal={} evidence={}",
                    session_id,
                    display_optional_i32(*exit_code),
                    display_optional_i32(*signal),
                    evidence
                )?,
                RuntimeEvent::Lost {
                    session_id,
                    evidence,
                } => writeln!(
                    f,
                    "runtime event=Lost session_id={session_id} evidence={evidence}"
                )?,
            }
        }
        Ok(())
    }
}

impl CliOutput for EventsPayload {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        self.events.render_human(f)?;
        writeln!(f, "cursor: {}", self.cursor)
    }
}

impl CliOutput for PaneSnapshot {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        write!(
            f,
            "pane snapshot; captured_at_ms={} scrollback_lines_requested={} scrollback_lines_included={} pane_history_lines={}\n{}",
            self.captured_at_ms,
            self.scrollback_lines_requested,
            self.scrollback_lines_included,
            self.pane_history_lines,
            self.content
        )
    }
}

impl CliOutput for RuntimeResponse {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        match self {
            Self::Spawned(payload) => writeln!(
                f,
                "spawn OK; lifecycle state={}; runtime event={}; runtime_pid={} log_dir={} stdout_path={} stderr_path={}",
                payload.lifecycle.state,
                event_name(&payload.event),
                display_optional_u32(payload.lifecycle.runtime_pid),
                display_optional_path(payload.log_dir.as_deref()),
                display_optional_path(payload.stdout_path.as_deref()),
                display_optional_path(payload.stderr_path.as_deref())
            ),
            other => write!(f, "unexpected runtime response: {other:?}"),
        }
    }
}

impl CliOutput for KillByPidResponse {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        writeln!(
            f,
            "kill OK; pid={} signal={} killed_after_grace={}",
            self.pid, self.signal, self.killed_after_grace
        )
    }
}

impl CliOutput for KillOutcome {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        match self {
            Self::Signalled => writeln!(f, "signalled"),
            Self::AlreadyExited => writeln!(f, "already exited"),
        }
    }
}

impl CliOutput for KilledPayload {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        self.outcome.render_human(f)
    }
}

impl CliOutput for NudgeResponse {
    fn render_human(&self, f: &mut impl Write) -> fmt::Result {
        match self.outcome {
            NudgeOutcome::Delivered => writeln!(f, "nudge delivered"),
            NudgeOutcome::Unsupported(reason) => {
                writeln!(f, "nudge unsupported; reason={}", reason.as_str())
            }
            NudgeOutcome::Failed(reason) => {
                writeln!(f, "nudge failed; reason={}", reason.as_str())
            }
        }
    }
}

fn print_lifecycle_counts(f: &mut impl Write, counts: &LifecycleCounts) -> fmt::Result {
    writeln!(f, "lifecycles")?;
    writeln!(f, "  forking             {}", counts.forking)?;
    writeln!(f, "  running             {}", counts.running)?;
    writeln!(f, "  exited              {}", counts.exited)?;
    writeln!(f, "  lost                {}", counts.lost)
}

fn print_recent_lost(f: &mut impl Write, doctor: &DoctorResponse) -> fmt::Result {
    if doctor.recent_lost.is_empty() {
        return writeln!(f, "recent lost           (none in last 24h)");
    }
    writeln!(f, "recent lost")?;
    for event in &doctor.recent_lost {
        writeln!(
            f,
            "  {} {} {}",
            event.session_id,
            event.evidence,
            event.occurred_at.to_rfc3339()
        )?;
    }
    Ok(())
}

fn format_migrations(values: &[String]) -> String {
    if values.is_empty() {
        return "none".to_owned();
    }
    values.join(", ")
}

fn format_capabilities(values: &[RuntimeCapability]) -> String {
    if values.is_empty() {
        return "none".to_owned();
    }
    values
        .iter()
        .map(|capability| capability.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_tmux(doctor: &DoctorResponse) -> String {
    if doctor.tmux.available {
        let version = doctor.tmux.version.as_deref().unwrap_or("version unknown");
        return format!("available ({version})");
    }
    match doctor.tmux.error.as_deref() {
        Some(error) => format!("unavailable ({error})"),
        None => "unavailable".to_owned(),
    }
}

fn format_readiness(readiness: &crate::DockerReadiness) -> String {
    if readiness.ready {
        return readiness
            .detail
            .as_deref()
            .map_or_else(|| "ready".to_owned(), |detail| format!("ready ({detail})"));
    }
    readiness.error.as_deref().map_or_else(
        || "unavailable".to_owned(),
        |error| format!("unavailable ({error})"),
    )
}

fn format_log_availability(value: Option<&LogAvailability>) -> String {
    match value {
        Some(LogAvailability::Headless { .. }) => "headless".to_owned(),
        Some(LogAvailability::TmuxPaneSnapshot) => "tmux_pane_snapshot".to_owned(),
        Some(LogAvailability::Unavailable { reason }) => format!("unavailable:{}", reason.as_str()),
        None => "-".to_owned(),
    }
}

fn format_duration(total_seconds: u64) -> String {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn event_name(event: &RuntimeEvent) -> &'static str {
    match event {
        RuntimeEvent::Running { .. } => "Running",
        RuntimeEvent::Terminated { .. } => "Terminated",
        RuntimeEvent::Lost { .. } => "Lost",
    }
}

fn display_optional_u32(value: Option<u32>) -> String {
    value.map_or_else(|| "-".to_owned(), |inner| inner.to_string())
}

fn display_optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "-".to_owned(), |inner| inner.to_string())
}

fn display_optional_path(value: Option<&Path>) -> String {
    value.map_or_else(|| "-".to_owned(), |path| path.display().to_string())
}
