#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use common::{RtmHarness, output_stdout, wait_until};
use tempfile::TempDir;
use uuid::Uuid;

const E2E_ENV: &str = "RTM_E2E_DOCKER";

#[test]
fn real_docker_spawn_lifecycle_is_opt_in() {
    if !opted_in() {
        eprintln!("skipping real Docker E2E; set {E2E_ENV}=1 to run");
        return;
    }
    if !docker_available() {
        eprintln!("skipping real Docker E2E; docker CLI or daemon is unavailable");
        return;
    }

    let session_id = Uuid::now_v7();
    let container = format!("rtm-{session_id}");
    let temp = TempDir::new().expect("temp dir");
    let images = DockerImages::new(session_id);
    build_base_image(&images, &workspace_root());
    build_e2e_image(&images, temp.path());

    let harness = RtmHarness::start_with_docker_image(&images.e2e);
    let _container_guard = ContainerGuard::new(container.clone());
    spawn_docker_runtime(&harness, &session_id, &images.e2e, temp.path());

    wait_for_container(&container);
    let top = docker_top(&container);
    assert!(
        top.contains("1001") && top.contains("claude"),
        "docker top did not show claude as the image user:\n{top}"
    );

    kill_runtime(&harness, &session_id);
    wait_for_container_absent(&container);
}

#[test]
fn real_docker_spawn_remaps_workdir_when_mount_covers_cwd() {
    if !opted_in() {
        eprintln!("skipping real Docker E2E; set {E2E_ENV}=1 to run");
        return;
    }
    if !docker_available() {
        eprintln!("skipping real Docker E2E; docker CLI or daemon is unavailable");
        return;
    }

    let session_id = Uuid::now_v7();
    let container = format!("rtm-{session_id}");
    let temp = TempDir::new().expect("temp dir");
    let mount_source = temp.path().join("helioy");
    let cwd = mount_source.join("littleorgans");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let images = DockerImages::new(session_id);
    build_base_image(&images, &workspace_root());
    build_e2e_image(&images, temp.path());

    let harness = RtmHarness::start_with_docker_image(&images.e2e);
    let _container_guard = ContainerGuard::new(container.clone());
    spawn_docker_runtime_with_mount(
        &harness,
        &session_id,
        &images.e2e,
        &cwd,
        &mount_source,
        "/workspace",
    );

    wait_for_container(&container);
    assert_eq!(docker_workdir(&container), "/workspace/littleorgans");

    kill_runtime(&harness, &session_id);
    wait_for_container_absent(&container);
}

fn opted_in() -> bool {
    std::env::var(E2E_ENV).as_deref() == Ok("1")
}

fn docker_available() -> bool {
    Command::new("docker")
        .arg("ps")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn build_base_image(images: &DockerImages, workspace_root: &Path) {
    let dockerfile = workspace_root.join("examples/dockerfiles/claude.Dockerfile");
    let context = workspace_root.join("examples/dockerfiles");
    let output = Command::new("docker")
        .args(["build", "-f"])
        .arg(dockerfile)
        .args(["-t", &images.base])
        .arg(context)
        .output()
        .expect("docker build base image");
    assert_success(output, "docker build base image");
}

fn build_e2e_image(images: &DockerImages, dir: &Path) {
    let dockerfile = dir.join("Dockerfile.e2e");
    std::fs::write(&dockerfile, e2e_dockerfile(&images.base)).expect("write e2e Dockerfile");
    let output = Command::new("docker")
        .args(["build", "-f"])
        .arg(&dockerfile)
        .args(["-t", &images.e2e])
        .arg(dir)
        .output()
        .expect("docker build e2e image");
    assert_success(output, "docker build e2e image");
}

fn e2e_dockerfile(base: &str) -> String {
    format!(
        r#"FROM {base}
USER root
RUN cat > /usr/local/bin/claude <<'EOF' && chmod +x /usr/local/bin/claude
#!/usr/bin/env bash
set -euo pipefail
trap 'exit 0' TERM INT
echo "rtm docker e2e ready"
while true; do sleep 1; done
EOF
USER rtm
"#
    )
}

fn spawn_docker_runtime(harness: &RtmHarness, session_id: &Uuid, image: &str, cwd: &Path) {
    let session_id = session_id.to_string();
    let output = harness
        .rtm_command()
        .args(["spawn", "--runtime", "claude", "--session-id", &session_id])
        .args(["--target", "headless", "--isolation", "docker"])
        .args(["--image", image, "--cwd"])
        .arg(cwd)
        .output()
        .expect("rtm spawn");
    assert_success(output, "rtm spawn docker");
}

fn spawn_docker_runtime_with_mount(
    harness: &RtmHarness,
    session_id: &Uuid,
    image: &str,
    cwd: &Path,
    mount_source: &Path,
    mount_target: &str,
) {
    let mount = format!("{}:{mount_target}:rw", mount_source.display());
    let session_id = session_id.to_string();
    let output = harness
        .rtm_command()
        .args(["spawn", "--runtime", "claude", "--session-id", &session_id])
        .args(["--target", "headless", "--isolation", "docker"])
        .args([
            "--image",
            image,
            "--env",
            "CLAUDE_CODE_OAUTH_TOKEN=e2e-token",
            "--cwd",
        ])
        .arg(cwd)
        .args(["--mount", &mount])
        .output()
        .expect("rtm spawn");
    assert_success(output, "rtm spawn docker with cwd cover");
}

fn wait_for_container(container: &str) {
    wait_until(Duration::from_secs(30), || {
        container_present(container).then_some(())
    })
    .unwrap_or_else(|| panic!("container {container} never appeared in docker ps"));
}

fn wait_for_container_absent(container: &str) {
    wait_until(Duration::from_secs(30), || {
        (!container_present(container)).then_some(())
    })
    .unwrap_or_else(|| panic!("container {container} remained present after kill"));
}

fn container_present(container: &str) -> bool {
    docker_ps_name(container)
        .lines()
        .any(|line| line.trim() == container)
}

fn docker_ps_name(container: &str) -> String {
    let output = Command::new("docker")
        .args(["ps", "--filter", &format!("name={container}"), "--format"])
        .arg("{{.Names}}")
        .output()
        .expect("docker ps");
    assert_success(output, "docker ps")
}

fn docker_top(container: &str) -> String {
    let output = Command::new("docker")
        .args(["top", container])
        .output()
        .expect("docker top");
    assert_success(output, "docker top")
}

fn docker_workdir(container: &str) -> String {
    let output = Command::new("docker")
        .args(["inspect", "--format"])
        .arg("{{.Config.WorkingDir}}")
        .arg(container)
        .output()
        .expect("docker inspect");
    assert_success(output, "docker inspect").trim().to_owned()
}

fn kill_runtime(harness: &RtmHarness, session_id: &Uuid) {
    let output = harness
        .rtm_command()
        .args(["kill", &session_id.to_string()])
        .output()
        .expect("rtm kill");
    assert_success(output, "rtm kill");
}

fn assert_success(output: Output, label: &str) -> String {
    let success = output.status.success();
    if success {
        return output_stdout(output);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    panic!("{label} failed; stdout={stdout:?}; stderr={stderr:?}");
}

struct DockerImages {
    base: String,
    e2e: String,
}

impl DockerImages {
    fn new(session_id: Uuid) -> Self {
        Self {
            base: format!("runtime-matters-claude:e2e-{session_id}-base"),
            e2e: format!("runtime-matters-claude:e2e-{session_id}"),
        }
    }
}

impl Drop for DockerImages {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rmi", "-f", &self.e2e])
            .output();
        let _ = Command::new("docker")
            .args(["rmi", "-f", &self.base])
            .output();
    }
}

struct ContainerGuard {
    name: String,
}

impl ContainerGuard {
    fn new(name: String) -> Self {
        Self { name }
    }
}

impl Drop for ContainerGuard {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.name])
            .output();
    }
}
