use std::sync::Arc;

use anyhow::Result;
use chrono::{Duration, Utc};
use lilo_rm_core::{
    DockerIsolationStatus, DockerReadiness, DockerStatus, DoctorResponse, HeadlessSpawnTarget,
    IsolationPolicy, LauncherStatus, SpawnRequest, SpawnTarget, TmuxStatus,
};
use tokio::process::Command;
use uuid::Uuid;

use crate::server::ServerState;

const RECENT_LOST_WINDOW: Duration = Duration::hours(24);

pub(crate) async fn collect(state: Arc<ServerState>) -> Result<DoctorResponse> {
    Ok(DoctorResponse {
        version: crate::version::runtime_version_info(),
        socket_path: state.config().endpoint.display_label(),
        uptime_secs: state.uptime_secs(),
        sqlite: state.store().migration_state().await?,
        lifecycles: state.store().lifecycle_counts().await?,
        watchers: state.watcher_counts().await,
        launchers: launcher_statuses(),
        tmux: tmux_status().await,
        docker: Box::new(docker_status().await),
        log_availability: state.log_availability_statuses().await,
        last_probe_sweep: state.store().last_probe_sweep().await?,
        recent_lost: state
            .store()
            .recent_lost_since(Utc::now() - RECENT_LOST_WINDOW)
            .await?,
    })
}

fn launcher_statuses() -> Vec<LauncherStatus> {
    lilo_runtime_launchers::registered_launchers()
        .into_iter()
        .map(launcher_status)
        .collect()
}

fn launcher_status(launcher: &'static dyn lilo_rm_core::RuntimeLauncher) -> LauncherStatus {
    let runtime = launcher.kind();
    let request = SpawnRequest {
        session_id: Uuid::nil(),
        runtime: runtime.clone(),
        isolation: IsolationPolicy::default(),
        image: None,
        env: Vec::new(),
        mounts: Vec::new(),
        cwd: lilo_rm_core::launcher_probe_cwd(),
        target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
        force: false,
        shell_resume: None,
    };
    match launcher.argv(&request) {
        Ok(argv) => LauncherStatus {
            runtime: runtime.to_string(),
            command: argv.first().cloned(),
            error: None,
        },
        Err(error) => LauncherStatus {
            runtime: runtime.to_string(),
            command: None,
            error: Some(error.to_string()),
        },
    }
}

async fn docker_status() -> DockerStatus {
    DockerStatus {
        cli: command_status("docker", &["--version"], "docker CLI").await,
        daemon: command_status(
            "docker",
            &["version", "--format", "{{.Server.Version}}"],
            "docker daemon",
        )
        .await,
        manifest_validation: command_status(
            "docker",
            &["manifest", "inspect", "--help"],
            "docker manifest inspect",
        )
        .await,
        isolation: DockerIsolationStatus {
            supported: true,
            default_workspace: "/workspace".to_owned(),
            experimental: true,
        },
    }
}

async fn command_status(command: &str, args: &[&str], label: &str) -> DockerReadiness {
    match Command::new(command).args(args).output().await {
        Ok(output) if output.status.success() => {
            DockerReadiness::ready(command_detail(&output.stdout, label))
        }
        Ok(output) => DockerReadiness::unavailable(command_error(&output.stderr, label)),
        Err(error) => DockerReadiness::unavailable(error.to_string()),
    }
}

fn command_detail(stdout: &[u8], label: &str) -> String {
    let detail = String::from_utf8_lossy(stdout).trim().to_owned();
    if detail.is_empty() {
        format!("{label} is available")
    } else {
        detail
    }
}

fn command_error(stderr: &[u8], label: &str) -> String {
    let message = String::from_utf8_lossy(stderr).trim().to_owned();
    if message.is_empty() {
        format!("{label} check failed without stderr")
    } else {
        message
    }
}

async fn tmux_status() -> TmuxStatus {
    match lilo_runtime_platform::tmux::TmuxGateway::version().await {
        Ok(Some(version)) => TmuxStatus {
            available: true,
            version: Some(version),
            error: None,
        },
        Ok(None) => TmuxStatus {
            available: false,
            version: None,
            error: None,
        },
        Err(error) => TmuxStatus {
            available: false,
            version: None,
            error: Some(error.to_string()),
        },
    }
}
