mod common;
use common::OrPanic as _;

#[test]
fn delete_session_help_exposes_session_flags() {
    for noun in ["session", "sessions"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_sm"))
            .args(["delete", noun, "--help"])
            .output()
            .or_panic("sm delete session help executes");

        assert_success(&format!("sm delete {noun} --help"), &output);
        let stdout = stdout(&output);
        assert!(stdout.contains("<SELECTOR>"));
        assert!(stdout.contains("Session selector used for matching sessions to terminate."));
        assert!(stdout.contains("namespace:<slug>"));
        assert!(stdout.contains("--namespace"));
        assert!(stdout.contains("Namespace scope for resolving session selectors"));
        assert!(stdout.contains("--all-namespaces"));
        assert!(stdout.contains("Resolve session selectors across all namespaces"));
        assert!(stdout.contains("--signal"));
        assert!(stdout.contains("--grace"));
    }
}

#[test]
fn delete_session_rejects_selector_flag_near_miss() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sm"))
        .args(["delete", "session", "--selector", "namespace:default"])
        .output()
        .or_panic("sm delete session near miss executes");

    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("unexpected argument '--selector'"));
    assert!(stderr.contains("Usage: sm delete session [OPTIONS] <SELECTOR>"));
}

#[test]
fn delete_rejects_agent_nouns() {
    for noun in ["agent", "agents"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_sm"))
            .args(["delete", noun, "all"])
            .output()
            .or_panic("sm delete agent executes");

        assert!(!output.status.success());
        assert!(stderr(&output).contains("unrecognized subcommand"));
    }
}

fn assert_success(command: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{command} failed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}
