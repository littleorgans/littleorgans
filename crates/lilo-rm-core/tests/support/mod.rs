#![allow(dead_code)]

use chrono::{TimeZone, Utc};
use lilo_rm_core::{
    DockerIsolationStatus, DockerReadiness, DockerStatus, DoctorResponse, LaunchEnv, LaunchSpec,
    LauncherStatus, Lifecycle, LifecycleCounts, LifecycleLogAvailability, LogAvailability,
    LostEvidence, MigrationState, PaneSnapshot, RecentLostEvent, RuntimeKind, ShimReady,
    TmuxStatus, VersionInfo, WatcherCounts, version_info,
};
use uuid::Uuid;

pub fn ready(session_id: Uuid) -> ShimReady {
    ShimReady {
        session_id,
        shim_pid: 4241,
        runtime_pid: 4242,
        start_time: timestamp(),
        tmux_pane: Some("rtm:0.1".parse().expect("pane")),
    }
}

pub fn headless_lifecycle(session_id: Uuid) -> Lifecycle {
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    assert!(lifecycle.mark_running(ready(session_id)));
    lifecycle.log_availability = Some(LogAvailability::Headless {
        stdout_path: "/tmp/rtm/logs/018f6e28-0000-7000-8000-000000000001/stdout.log".into(),
        stderr_path: "/tmp/rtm/logs/018f6e28-0000-7000-8000-000000000001/stderr.log".into(),
    });
    lifecycle
}

pub fn tmux_lifecycle(session_id: Uuid) -> Lifecycle {
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    assert!(lifecycle.mark_running(ready(session_id)));
    lifecycle.log_availability = Some(LogAvailability::TmuxPaneSnapshot);
    lifecycle
}

pub fn launch_spec() -> LaunchSpec {
    LaunchSpec {
        argv: vec!["claude".to_owned(), "--resume".to_owned()],
        env: vec![LaunchEnv::new("RTM", "1")],
        cwd: "/tmp/rtm".into(),
        shell_resume: None,
    }
}

pub fn doctor_response() -> DoctorResponse {
    DoctorResponse {
        version: test_version_info(),
        socket_path: "/tmp/rtmd.sock".to_owned(),
        uptime_secs: 12,
        sqlite: MigrationState {
            applied: 3,
            total: 3,
            applied_descriptions: vec![
                "lifecycle".to_owned(),
                "probe state".to_owned(),
                "lifecycle isolation".to_owned(),
            ],
            pending_descriptions: Vec::new(),
        },
        lifecycles: LifecycleCounts {
            forking: 1,
            running: 2,
            exited: 3,
            lost: 4,
        },
        watchers: watcher_counts(),
        launchers: vec![LauncherStatus {
            runtime: "claude".to_owned(),
            command: Some("claude".to_owned()),
            error: None,
        }],
        tmux: TmuxStatus {
            available: true,
            version: Some("tmux 3.5a".to_owned()),
            error: None,
        },
        docker: Box::new(docker_status()),
        log_availability: vec![LifecycleLogAvailability {
            session_id: session_id(),
            log_availability: LogAvailability::TmuxPaneSnapshot,
        }],
        last_probe_sweep: Some(timestamp()),
        recent_lost: vec![RecentLostEvent {
            session_id: other_session_id(),
            evidence: LostEvidence::PidNotAlive,
            occurred_at: timestamp(),
        }],
    }
}

pub fn docker_status() -> DockerStatus {
    DockerStatus {
        cli: DockerReadiness::ready("Docker version 27.0.0"),
        daemon: DockerReadiness::ready("27.0.0"),
        manifest_validation: DockerReadiness::ready("docker manifest inspect is available"),
        isolation: DockerIsolationStatus {
            supported: true,
            default_workspace: "/workspace".to_owned(),
            experimental: true,
        },
    }
}

pub fn watcher_counts() -> WatcherCounts {
    WatcherCounts {
        process_exit_watchers: 5,
        shim_sockets: 6,
        event_waiters: 3,
    }
}

pub fn pane_snapshot() -> PaneSnapshot {
    PaneSnapshot {
        content: "\u{1b}[32mhello\u{1b}[0m".to_owned(),
        captured_at_ms: 1_700_000_001_000,
        scrollback_lines_requested: 1000,
        scrollback_lines_included: 1,
        pane_history_lines: 2000,
    }
}

pub fn test_version_info() -> VersionInfo {
    VersionInfo::new("0.1.6", "0123456")
}

pub fn current_version_info() -> VersionInfo {
    version_info()
}

pub fn session_id() -> Uuid {
    Uuid::parse_str("018f6e28-0000-7000-8000-000000000001").expect("uuid")
}

pub fn other_session_id() -> Uuid {
    Uuid::parse_str("018f6e28-0000-7000-8000-000000000002").expect("uuid")
}

pub fn timestamp() -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap()
}
