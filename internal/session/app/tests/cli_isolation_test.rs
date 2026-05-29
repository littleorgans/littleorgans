mod common;
use common::{OrPanic as _, assert_success};

use std::process::Command;

const IMAGE: &str = "runtime-matters-claude:local";

const FAKE_DOCKER_SCRIPT: &str = r#"#!/bin/sh
if [ "$1" = "version" ]; then
  printf '25.0.0\n'
  exit 0
fi

if [ "$1" = "image" ] && [ "$2" = "inspect" ]; then
  case "$5" in
    "{{json .Config.User}}")
      printf '"agent"\n'
      exit 0
      ;;
    "{{json .Architecture}}")
      printf '"arm64"\n'
      exit 0
      ;;
  esac
fi

if [ "$1" = "manifest" ] && [ "$2" = "inspect" ]; then
  printf '{"manifests":[{"platform":{"architecture":"arm64"}}]}\n'
  exit 0
fi

if [ "$1" = "container" ] && [ "$2" = "inspect" ]; then
  printf 'true\n'
  exit 0
fi

if [ "$1" = "run" ]; then
  trap 'exit 0' TERM INT
  while :; do sleep 60; done
fi

printf 'unexpected docker invocation: %s\n' "$*" >&2
exit 1
"#;

#[test]
fn run_accepts_docker_isolation_and_preserves_host_default() {
    let runtime_path = common::fake_runtime_path("claude");
    common::write_fake_command(runtime_path.path(), "docker", FAKE_DOCKER_SCRIPT)
        .or_panic("fake docker command writes");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());
    let project = daemon.dir.path().join("project");
    std::fs::create_dir_all(&project).or_panic("project dir");
    let project_arg = project.display().to_string();

    for (command, extra_args) in [
        (
            "sm run with docker isolation",
            vec!["--isolation", "docker", "--image", IMAGE],
        ),
        ("sm run with host default", Vec::new()),
    ] {
        let output = daemon
            .command()
            .args(["run", "claude", "--role", "x", "--dir", &project_arg])
            .args(extra_args)
            .args(["--detach"])
            .output()
            .or_panic("sm run executes");

        assert_success(command, &output);
    }
}

#[test]
fn run_rejects_unknown_isolation_at_clap() {
    let project = tempfile::tempdir().or_panic("project dir");
    let output = Command::new(env!("CARGO_BIN_EXE_sm"))
        .args([
            "run",
            "claude",
            "--isolation",
            "kubernetes",
            "--role",
            "x",
            "--dir",
            &project.path().display().to_string(),
        ])
        .output()
        .or_panic("sm run executes");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid isolation policy kubernetes"));
    assert!(stderr.contains("expected host, docker, or docker:PROFILE"));
}

#[test]
fn run_rejects_mount_with_host_isolation_before_daemon() {
    let project = tempfile::tempdir().or_panic("project dir");
    let output = Command::new(env!("CARGO_BIN_EXE_sm"))
        .args([
            "run",
            "claude",
            "--role",
            "x",
            "--dir",
            &project.path().display().to_string(),
            "--mount",
            "/host/config:/container/config",
        ])
        .output()
        .or_panic("sm run executes");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--mount is docker-only and cannot be used with --isolation host"));
}
