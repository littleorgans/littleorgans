use anyhow::{Result, bail};
use lilo_rm_core::CliOutput;
use lilo_session_core::{DoctorRequest, RpcResponse, RuntimeDoctorReport, SessionRpc};
use std::fmt::Write as _;

use crate::cli::cli_def::DoctorArgs;

pub async fn run(_args: DoctorArgs) -> Result<()> {
    let response = crate::cli::client::send_request(&SessionRpc::Doctor {
        request: DoctorRequest::default(),
    })
    .await?;

    match response {
        RpcResponse::Doctor { response } => {
            let status = response.status.clone();
            println!("session-matters");
            println!("  status: {}", response.status);
            println!("  runtime: {}", response.runtime);
            for finding in response.findings {
                println!(
                    "  {} {} {}",
                    finding.severity,
                    finding.session_id.unwrap_or_else(|| "-".to_string()),
                    finding.message
                );
            }
            print_runtime_matters(&response.runtime_matters)?;
            if status == "ok" {
                Ok(())
            } else {
                bail!("doctor reported {status}")
            }
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

fn print_runtime_matters(report: &RuntimeDoctorReport) -> Result<()> {
    print!("{}", render_runtime_matters(report)?);
    Ok(())
}

fn render_runtime_matters(report: &RuntimeDoctorReport) -> Result<String> {
    let mut output = String::new();
    writeln!(output, "runtime-matters")?;
    writeln!(output, "  status: {}", report.status)?;
    if let Some(doctor) = &report.doctor {
        let mut rendered = String::new();
        doctor.render_human(&mut rendered)?;
        for line in rendered.lines() {
            writeln!(output, "  {line}")?;
        }
        return Ok(output);
    }
    if let Some(socket_path) = &report.socket_path {
        writeln!(output, "  socket: {socket_path}")?;
    }
    if let Some(code) = &report.code {
        writeln!(output, "  code: {code}")?;
    }
    if let Some(message) = &report.message {
        writeln!(output, "  message: {message}")?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilo_rm_core::{
        DockerStatus, LifecycleCounts, MigrationState, TmuxStatus, WatcherCounts, version_info,
    };

    #[test]
    fn runtime_matters_render_is_equal_when_transport_fields_leave_report() {
        let doctor = runtime_doctor_response();
        let old_report = RuntimeDoctorReport {
            status: "ok".to_string(),
            doctor: Some(Box::new(doctor.clone())),
            socket_path: Some("/tmp/outer-rtmd.sock".to_string()),
            code: Some("runtime_unavailable".to_string()),
            message: None,
        };
        let new_report = RuntimeDoctorReport {
            status: "ok".to_string(),
            doctor: Some(Box::new(doctor)),
            socket_path: None,
            code: None,
            message: None,
        };

        let old_rendered = render_runtime_matters(&old_report).expect("old report renders");
        let new_rendered = render_runtime_matters(&new_report).expect("new report renders");

        assert_eq!(old_rendered.as_bytes(), new_rendered.as_bytes());
        assert!(!new_rendered.contains("/tmp/outer-rtmd.sock"));
        assert!(!new_rendered.contains("runtime_unavailable"));
    }

    fn runtime_doctor_response() -> lilo_rm_core::DoctorResponse {
        lilo_rm_core::DoctorResponse {
            version: version_info(),
            socket_path: "/tmp/domain-rtmd.sock".to_string(),
            uptime_secs: 7,
            sqlite: MigrationState {
                applied: 1,
                total: 1,
                applied_descriptions: vec!["init".to_string()],
                pending_descriptions: Vec::new(),
            },
            lifecycles: LifecycleCounts::default(),
            watchers: WatcherCounts {
                process_exit_watchers: 0,
                shim_sockets: 0,
                event_waiters: 0,
            },
            launchers: Vec::new(),
            tmux: TmuxStatus {
                available: false,
                version: None,
                error: Some("tmux unavailable in test".to_string()),
            },
            docker: Box::new(DockerStatus::legacy_missing()),
            log_availability: Vec::new(),
            last_probe_sweep: None,
            recent_lost: Vec::new(),
        }
    }
}
