#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::process::Command;

use uuid::Uuid;

#[test]
fn root_version_flag_prints_runtime_matters_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_rtm"))
        .arg("--version")
        .output()
        .expect("rtm --version");

    assert!(output.status.success(), "rtm --version failed: {output:?}");
    assert!(output.stderr.is_empty(), "stderr was not empty: {output:?}");

    let stdout = String::from_utf8(output.stdout).expect("version output utf8");
    let expected = format!("runtime-matters {}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(stdout, expected);
}

#[test]
fn spawn_help_documents_cwd_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_rtm"))
        .args(["spawn", "--help"])
        .output()
        .expect("rtm spawn --help");

    assert!(
        output.status.success(),
        "rtm spawn --help failed: {output:?}"
    );
    assert!(output.stderr.is_empty(), "stderr was not empty: {output:?}");

    let stdout = String::from_utf8(output.stdout).expect("help output utf8");
    assert!(stdout.contains("--cwd <PATH>"), "{stdout}");
    assert!(
        stdout.contains("--isolation <host|docker[:PROFILE]>"),
        "{stdout}"
    );
    assert!(stdout.contains("--image <IMAGE>"), "{stdout}");
    assert!(stdout.contains("--env <KEY[=VALUE]>"), "{stdout}");
    assert!(
        stdout.contains("--mount <HOST:CONTAINER[:ro|:rw]>"),
        "{stdout}"
    );
    assert!(stdout.contains("defaults to :ro"), "{stdout}");
    assert!(stdout.contains("rejects --isolation host"), "{stdout}");
}

#[test]
fn nudge_help_documents_typed_failure_reasons() {
    let output = Command::new(env!("CARGO_BIN_EXE_rtm"))
        .args(["nudge", "--help"])
        .output()
        .expect("rtm nudge --help");

    assert!(
        output.status.success(),
        "rtm nudge --help failed: {output:?}"
    );
    assert!(output.stderr.is_empty(), "stderr was not empty: {output:?}");

    let stdout = String::from_utf8(output.stdout).expect("help output utf8");
    assert!(stdout.contains("headless_lifecycle"), "{stdout}");
    assert!(stdout.contains("session_ended"), "{stdout}");
    assert!(stdout.contains("tmux_pane_dead"), "{stdout}");
}

#[test]
fn spawn_cwd_flag_rejects_missing_path_before_daemon_request() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let missing = temp.path().join("missing");
    let output = spawn_with_cwd(&missing);

    assert!(!output.status.success(), "spawn unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        stderr.contains(&format!("spawn cwd does not exist: {}", missing.display())),
        "{stderr}"
    );
}

#[test]
fn spawn_cwd_flag_rejects_file_path_before_daemon_request() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let file = temp.path().join("file");
    std::fs::write(&file, "").expect("file");
    let file = std::fs::canonicalize(file).expect("canonical file");
    let output = spawn_with_cwd(&file);

    assert!(!output.status.success(), "spawn unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        stderr.contains(&format!("spawn cwd is not a directory: {}", file.display())),
        "{stderr}"
    );
}

#[test]
fn spawn_isolation_flag_rejects_invalid_policy_before_daemon_request() {
    let output = spawn_command()
        .args(["--isolation", "sandbox"])
        .output()
        .expect("rtm spawn");

    assert!(!output.status.success(), "spawn unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        stderr.contains("invalid isolation policy sandbox"),
        "{stderr}"
    );
}

#[test]
fn spawn_env_flag_rejects_missing_caller_env_before_daemon_request() {
    let key = format!("RTM_TEST_MISSING_{}", Uuid::now_v7().simple());
    let output = spawn_command()
        .args(["--env", &key])
        .env_remove(&key)
        .output()
        .expect("rtm spawn");

    assert!(!output.status.success(), "spawn unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        stderr.contains(&format!("spawn env {key} is not set in caller environment")),
        "{stderr}"
    );
}

#[test]
fn spawn_mount_flag_rejects_host_isolation_before_daemon_request() {
    let output = spawn_with_mount("/host:/container");

    assert!(!output.status.success(), "spawn unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        stderr.contains("--mount is docker-only and cannot be used with --isolation host"),
        "{stderr}"
    );
}

#[test]
fn spawn_mount_flag_rejects_malformed_values_before_daemon_request() {
    for (value, expected) in [
        ("/host", "mount value is missing ':'"),
        (":/container", "mount host source cannot be empty"),
        ("/host:", "mount container target cannot be empty"),
        (
            "/host:/container:bogus",
            "unknown mount access mode bogus; expected ro or rw",
        ),
    ] {
        let output = spawn_with_mount(value);

        assert!(
            !output.status.success(),
            "spawn unexpectedly succeeded for {value}"
        );
        let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
        assert!(stderr.contains(expected), "{stderr}");
    }
}

#[test]
fn spawn_mount_flag_rejects_tilde_source_when_home_is_unset() {
    let output = spawn_command()
        .args(["--isolation", "docker", "--mount", "~/foo:/container/path"])
        .env_remove("HOME")
        .output()
        .expect("rtm spawn");

    assert!(!output.status.success(), "spawn unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        stderr.contains("mount source uses '~' but HOME is not set"),
        "{stderr}"
    );
}

fn spawn_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rtm"));
    command
        .arg("spawn")
        .arg("--runtime")
        .arg("claude")
        .arg("--session-id")
        .arg(Uuid::now_v7().to_string())
        .arg("--target")
        .arg("headless");
    command
}

fn spawn_with_mount(value: &str) -> std::process::Output {
    spawn_command()
        .args(["--mount", value])
        .output()
        .expect("rtm spawn")
}

fn spawn_with_cwd(path: &std::path::Path) -> std::process::Output {
    spawn_command()
        .arg("--cwd")
        .arg(path)
        .output()
        .expect("rtm spawn")
}
