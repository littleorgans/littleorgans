use lilo_rm_core::{
    DockerReadiness, DoctorResponse as RuntimeDoctorResponse, RUNTIME_PROTOCOL_CAPABILITIES,
    RUNTIME_PROTOCOL_VERSION,
};
use lilo_session_core::RuntimeDoctorReport;

pub(super) fn warnings_for_report(report: &RuntimeDoctorReport) -> Vec<String> {
    let mut warnings = Vec::new();
    if report.doctor.is_none() || report.status != "ok" {
        warnings.push(status_warning(report));
    }
    if let Some(doctor) = report.doctor.as_deref() {
        warnings.extend(detail_warnings(doctor));
    }
    warnings
}

fn status_warning(report: &RuntimeDoctorReport) -> String {
    let mut warning = if report.doctor.is_some() {
        format!("runtime doctor status {}", report.status)
    } else {
        format!("runtime doctor unavailable: status {}", report.status)
    };
    if let Some(code) = &report.code {
        warning.push_str(" code=");
        warning.push_str(code);
    }
    if let Some(message) = &report.message {
        warning.push_str(" message=");
        warning.push_str(message);
    }
    if let Some(socket_path) = &report.socket_path {
        warning.push_str(" socket=");
        warning.push_str(socket_path);
    }
    warning
}

fn detail_warnings(doctor: &RuntimeDoctorResponse) -> Vec<String> {
    let mut warnings = Vec::new();
    if doctor.version.protocol_version != RUNTIME_PROTOCOL_VERSION {
        warnings.push(format!(
            "runtime protocol mismatch: required {RUNTIME_PROTOCOL_VERSION}, got {}",
            doctor.version.protocol_version
        ));
    }

    let missing_capabilities = RUNTIME_PROTOCOL_CAPABILITIES
        .iter()
        .copied()
        .filter(|capability| !doctor.version.capabilities.contains(capability))
        .map(|capability| capability.to_string())
        .collect::<Vec<_>>();
    if !missing_capabilities.is_empty() {
        warnings.push(format!(
            "runtime protocol missing capabilities: {}",
            missing_capabilities.join(", ")
        ));
    }

    if !doctor.sqlite.pending_descriptions.is_empty() {
        warnings.push(format!(
            "runtime sqlite migrations pending: {}",
            doctor.sqlite.pending_descriptions.join(", ")
        ));
    }

    warnings.extend(
        doctor
            .launchers
            .iter()
            .filter_map(|launcher| launcher.error.as_ref().map(|error| (launcher, error)))
            .map(|(launcher, error)| {
                format!("runtime launcher {} unavailable: {error}", launcher.runtime)
            }),
    );
    if !doctor.tmux.available {
        warnings.push(format!(
            "runtime tmux unavailable{}",
            optional_detail(doctor.tmux.error.as_deref(), None)
        ));
    }
    warnings.extend(docker_warnings("cli", &doctor.docker.cli));
    warnings.extend(docker_warnings("daemon", &doctor.docker.daemon));
    warnings.extend(docker_warnings(
        "manifest validation",
        &doctor.docker.manifest_validation,
    ));
    if doctor.lifecycles.lost > 0 {
        warnings.push(format!(
            "runtime lifecycles lost: {}",
            doctor.lifecycles.lost
        ));
    }
    if !doctor.recent_lost.is_empty() {
        warnings.push(format!(
            "runtime recent lost sessions: {}",
            doctor.recent_lost.len()
        ));
    }
    warnings
}

fn docker_warnings(component: &str, readiness: &DockerReadiness) -> Option<String> {
    if readiness.ready {
        return None;
    }
    Some(format!(
        "runtime docker {component} not ready{}",
        optional_detail(readiness.error.as_deref(), readiness.detail.as_deref())
    ))
}

fn optional_detail(error: Option<&str>, detail: Option<&str>) -> String {
    error
        .or(detail)
        .map(|detail| format!(": {detail}"))
        .unwrap_or_default()
}

#[cfg(test)]
pub(super) fn response() -> RuntimeDoctorResponse {
    use lilo_rm_core::{
        DockerIsolationStatus, DockerStatus, LauncherStatus, LifecycleCounts, MigrationState,
        TmuxStatus, WatcherCounts,
    };

    RuntimeDoctorResponse {
        version: lilo_rm_core::version_info(),
        socket_path: "/tmp/rtmd.sock".to_string(),
        uptime_secs: 1,
        sqlite: MigrationState {
            applied: 0,
            total: 0,
            applied_descriptions: Vec::new(),
            pending_descriptions: Vec::new(),
        },
        lifecycles: LifecycleCounts {
            forking: 0,
            running: 0,
            exited: 0,
            lost: 0,
        },
        watchers: WatcherCounts {
            process_exit_watchers: 0,
            shim_sockets: 0,
            event_waiters: 0,
        },
        launchers: vec![LauncherStatus {
            runtime: "claude".to_string(),
            command: Some("claude".to_string()),
            error: None,
        }],
        tmux: TmuxStatus {
            available: true,
            version: Some("tmux 3.5".to_string()),
            error: None,
        },
        docker: Box::new(DockerStatus {
            cli: ready_docker_probe(),
            daemon: ready_docker_probe(),
            manifest_validation: ready_docker_probe(),
            isolation: DockerIsolationStatus {
                supported: true,
                default_workspace: "/tmp".to_string(),
                experimental: false,
            },
        }),
        log_availability: Vec::new(),
        last_probe_sweep: None,
        recent_lost: Vec::new(),
    }
}

#[cfg(test)]
fn ready_docker_probe() -> DockerReadiness {
    DockerReadiness {
        ready: true,
        detail: None,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_without_detail_surfaces_human_warning() {
        let report = RuntimeDoctorReport {
            status: "error".to_string(),
            doctor: None,
            socket_path: Some("/tmp/rtmd.sock".to_string()),
            code: Some("runtime_unreachable".to_string()),
            message: Some("connection refused".to_string()),
        };

        assert_eq!(
            warnings_for_report(&report),
            vec![
                "runtime doctor unavailable: status error code=runtime_unreachable message=connection refused socket=/tmp/rtmd.sock"
                    .to_string()
            ]
        );
    }

    #[test]
    fn detail_derives_substrate_warnings() {
        let mut doctor = response();
        doctor.version.protocol_version = "old".to_string();
        doctor.version.capabilities.clear();
        doctor.sqlite.pending_descriptions = vec!["add_runtime_events".to_string()];
        doctor.launchers[0].error = Some("not found".to_string());
        doctor.tmux.available = false;
        doctor.tmux.error = Some("tmux missing".to_string());
        doctor.docker.cli.ready = false;
        doctor.docker.cli.error = Some("docker missing".to_string());
        doctor.lifecycles.lost = 2;
        let report = RuntimeDoctorReport {
            status: "degraded".to_string(),
            doctor: Some(Box::new(doctor)),
            socket_path: Some("/tmp/rtmd.sock".to_string()),
            code: None,
            message: None,
        };

        let warnings = warnings_for_report(&report);

        for expected in [
            "runtime doctor status degraded socket=/tmp/rtmd.sock".to_string(),
            format!("runtime protocol mismatch: required {RUNTIME_PROTOCOL_VERSION}, got old"),
            "runtime sqlite migrations pending: add_runtime_events".to_string(),
            "runtime launcher claude unavailable: not found".to_string(),
            "runtime tmux unavailable: tmux missing".to_string(),
            "runtime docker cli not ready: docker missing".to_string(),
            "runtime lifecycles lost: 2".to_string(),
        ] {
            assert!(
                warnings.iter().any(|warning| warning == &expected),
                "missing {expected:?} in {warnings:#?}"
            );
        }
        assert!(
            warnings
                .iter()
                .any(|warning| warning.starts_with("runtime protocol missing capabilities: ")),
            "missing capability warning in {warnings:#?}"
        );
    }
}
