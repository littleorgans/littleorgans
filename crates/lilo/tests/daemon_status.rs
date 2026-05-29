use std::process::Command;

fn isolated_lilo() -> (tempfile::TempDir, Command) {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let socket = dir.path().join("lilod.sock");
    let mut command = Command::new(env!("CARGO_BIN_EXE_lilo"));
    command
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("LILO_HOME", home)
        .env("LILO_SOCKET_PATH", socket)
        .env("HOME", dir.path());
    (dir, command)
}

#[test]
fn daemon_status_is_a_pure_query_when_daemon_is_absent() {
    let (_dir, mut command) = isolated_lilo();
    let output = command
        .args(["daemon", "status"])
        .output()
        .expect("lilo daemon status");

    assert!(output.status.success(), "status output: {output:?}");
    assert!(output.stderr.is_empty(), "stderr was not empty: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(stdout.contains("lilod not running"), "{stdout}");
}

#[test]
fn daemon_status_wait_times_out_when_daemon_is_absent() {
    let (_dir, mut command) = isolated_lilo();
    let output = command
        .args(["daemon", "status", "--wait=0"])
        .output()
        .expect("lilo daemon status --wait");

    assert!(!output.status.success(), "wait output: {output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(stderr.contains("daemon was not ready"), "{stderr}");
}
