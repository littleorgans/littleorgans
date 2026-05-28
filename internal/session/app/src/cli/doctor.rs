use anyhow::{Result, bail};
use lilo_rm_core::CliOutput;
use lilo_session_core::{DoctorRequest, RpcResponse, RuntimeDoctorReport, SessionRpc};

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
    println!("runtime-matters");
    println!("  status: {}", report.status);
    if let Some(doctor) = &report.doctor {
        let mut rendered = String::new();
        doctor.render_human(&mut rendered)?;
        for line in rendered.lines() {
            println!("  {line}");
        }
        return Ok(());
    }
    if let Some(socket_path) = &report.socket_path {
        println!("  socket: {socket_path}");
    }
    if let Some(code) = &report.code {
        println!("  code: {code}");
    }
    if let Some(message) = &report.message {
        println!("  message: {message}");
    }
    Ok(())
}
