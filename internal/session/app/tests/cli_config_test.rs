mod common;
use common::OrPanic as _;

use std::process::{Command, Output};

#[test]
fn config_help_lists_set_context() {
    let output = Command::new(env!("CARGO_BIN_EXE_sm"))
        .args(["config", "--help"])
        .output()
        .or_panic("sm config --help executes");

    assert_success("sm config --help", &output);
    assert!(stdout(&output).contains("set-context"));
}

#[test]
fn set_context_writes_sm_home_binding_after_daemon_lookup() {
    let daemon = common::DaemonFixture::start();
    create_namespace(&daemon, "alpha");

    let output = daemon
        .command()
        .args(["config", "set-context", "alpha"])
        .output()
        .or_panic("sm config set-context executes");

    assert_success("sm config set-context alpha", &output);
    assert_eq!(binding_contents(daemon.dir.path()), "alpha\n");
}

#[test]
fn set_context_accepts_default_namespace() {
    let daemon = common::DaemonFixture::start();

    let output = daemon
        .command()
        .args(["config", "set-context", "default"])
        .output()
        .or_panic("sm config set-context default executes");

    assert_success("sm config set-context default", &output);
    assert_eq!(binding_contents(daemon.dir.path()), "default\n");
}

#[test]
fn set_context_rejects_unknown_namespace_without_write() {
    let daemon = common::DaemonFixture::start();

    let output = daemon
        .command()
        .args(["config", "set-context", "missing"])
        .output()
        .or_panic("sm config set-context missing executes");

    assert!(!output.status.success());
    assert!(stderr(&output).contains("unknown namespace: missing"));
    assert!(!daemon.dir.path().join("namespace").exists());
}

#[test]
fn set_context_uses_home_fallback_when_sm_home_is_unset() {
    let daemon = common::DaemonFixture::start();
    create_namespace(&daemon, "fallback");
    let home = tempfile::tempdir().or_panic("home tempdir");

    let output = Command::new(env!("CARGO_BIN_EXE_sm"))
        .args(["config", "set-context", "fallback"])
        .env_remove("SM_HOME")
        .env("HOME", home.path())
        .env("SM_SOCKET_PATH", daemon.socket_path())
        .output()
        .or_panic("sm config set-context fallback executes");

    assert_success("sm config set-context fallback", &output);
    assert_eq!(binding_contents(&home.path().join(".sm")), "fallback\n");
}

#[test]
fn set_context_overwrites_binding_atomically() {
    let daemon = common::DaemonFixture::start();
    create_namespace(&daemon, "alpha");
    create_namespace(&daemon, "beta");

    let alpha = daemon
        .command()
        .args(["config", "set-context", "alpha"])
        .output()
        .or_panic("sm config set-context alpha executes");
    assert_success("sm config set-context alpha", &alpha);

    let beta = daemon
        .command()
        .args(["config", "set-context", "beta"])
        .output()
        .or_panic("sm config set-context beta executes");

    assert_success("sm config set-context beta", &beta);
    assert_eq!(binding_contents(daemon.dir.path()), "beta\n");
    let temp_writes = std::fs::read_dir(daemon.dir.path())
        .or_panic("sm home can be listed")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".namespace."))
        .count();
    assert_eq!(temp_writes, 0);
}

#[test]
fn set_context_daemon_unreachable_does_not_write() {
    let sm_home = tempfile::tempdir().or_panic("sm home tempdir");

    let output = Command::new(env!("CARGO_BIN_EXE_sm"))
        .args(["config", "set-context", "default"])
        .env("SM_HOME", sm_home.path())
        .env("HOME", sm_home.path())
        .output()
        .or_panic("sm config set-context default executes");

    assert!(!output.status.success());
    assert!(stderr(&output).contains("failed to connect"));
    assert!(!sm_home.path().join("namespace").exists());
}

fn create_namespace(daemon: &common::DaemonFixture, name: &str) {
    let output = daemon
        .command()
        .args(["create", "namespace", name])
        .output()
        .or_panic("sm create namespace executes");
    assert_success("sm create namespace", &output);
}

fn binding_contents(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join("namespace")).or_panic("binding file reads")
}

fn assert_success(command: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{command} failed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}
