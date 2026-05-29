use std::process::Command;

#[test]
fn identity_whoami_requires_the_daemon_socket() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let socket = dir.path().join("missing.sock");
    let output = Command::new(env!("CARGO_BIN_EXE_lilo"))
        .args(["identity", "whoami"])
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("LILO_HOME", home)
        .env("LILO_SOCKET_PATH", socket)
        .env("HOME", dir.path())
        .output()
        .expect("lilo identity whoami");

    assert!(!output.status.success(), "whoami output: {output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(stderr.contains("failed to connect"), "{stderr}");
}
