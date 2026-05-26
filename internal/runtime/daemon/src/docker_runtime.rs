use std::path::Path;
use std::process::Command as StdCommand;

use anyhow::Result;
use lilo_rm_core::{
    IsolationProfile, KillOutcome, LaunchSpec, MountSpec, RuntimeSignal, SpawnTarget,
};
use tokio::process::Command;
use uuid::Uuid;

use crate::docker_argv::{self, container_name};
use crate::error::RuntimeFailure;

pub(crate) fn docker_run_launch(
    session_id: Uuid,
    profile: &IsolationProfile,
    image: &str,
    launch: &LaunchSpec,
    mounts: &[MountSpec],
    target: &SpawnTarget,
) -> Result<LaunchSpec> {
    docker_argv::docker_run_launch(
        session_id,
        profile,
        image,
        launch,
        mounts,
        target,
        &docker_command(),
    )
}

fn docker_command() -> String {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join("docker"))
        .find(|path| is_executable(path))
        .map_or_else(
            || "docker".to_owned(),
            |path| path.to_string_lossy().into_owned(),
        )
}

fn is_executable(path: &Path) -> bool {
    path.is_file()
}

pub(crate) struct DockerCliRuntime;

impl DockerCliRuntime {
    pub(crate) async fn running(&self, session_id: Uuid) -> Result<bool> {
        let output = Command::new("docker")
            .arg("container")
            .arg("inspect")
            .arg(container_name(session_id))
            .arg("--format")
            .arg("{{.State.Running}}")
            .output()
            .await
            .map_err(|error| RuntimeFailure::docker_unavailable(error.to_string()))?;

        if !output.status.success() {
            return Ok(false);
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
    }
}

pub(crate) fn container_running_blocking(session_id: Uuid) -> Result<bool> {
    let output = StdCommand::new("docker")
        .arg("container")
        .arg("inspect")
        .arg(container_name(session_id))
        .arg("--format")
        .arg("{{.State.Running}}")
        .output()
        .map_err(|error| RuntimeFailure::docker_unavailable(error.to_string()))?;

    if !output.status.success() {
        return Ok(false);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

pub(crate) async fn kill_container(session_id: Uuid, signal: RuntimeSignal) -> Result<KillOutcome> {
    let mut command = Command::new("docker");
    command.arg("kill");
    command
        .arg("--signal")
        .arg(signal_number_arg(signal))
        .arg(container_name(session_id));

    let output = command
        .output()
        .await
        .map_err(|error| RuntimeFailure::docker_unavailable(error.to_string()))?;

    if output.status.success() {
        return Ok(KillOutcome::Signalled);
    }
    if command_stderr(&output.stderr).contains("No such container") {
        return Ok(KillOutcome::AlreadyExited);
    }
    Err(RuntimeFailure::docker_unavailable(command_stderr(
        &output.stderr,
    )))
}

fn signal_number_arg(signal: RuntimeSignal) -> String {
    lilo_runtime_platform::signal::signal_number(signal).to_string()
}

fn command_stderr(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr).trim().to_owned();
    if message.is_empty() {
        "docker command failed without stderr".to_owned()
    } else {
        message
    }
}
