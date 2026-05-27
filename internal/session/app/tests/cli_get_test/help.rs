use crate::common::OrPanic as _;
use crate::{assert_success, stdout};

#[test]
pub(crate) fn get_session_help_exposes_only_session_read_arguments() {
    for resource in ["session", "sessions"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_sm"))
            .args(["get", resource, "--help"])
            .output()
            .or_panic("sm get session help executes");

        assert_success("sm get session help", &output);
        let stdout = stdout(&output);
        assert!(stdout.contains("--selector"));
        assert!(stdout.contains("Optional session selector used for matching sessions."));
        assert!(stdout.contains("--namespace"));
        assert!(stdout.contains("Namespace scope for resolving session selectors"));
        assert!(stdout.contains("--all-namespaces"));
        assert!(stdout.contains("--json"));
        assert!(stdout.contains("--show-labels"));
    }
}

#[test]
pub(crate) fn get_namespace_help_exposes_only_namespace_read_arguments() {
    for resource in ["namespace", "namespaces"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_sm"))
            .args(["get", resource, "--help"])
            .output()
            .or_panic("sm get namespace help executes");

        assert_success("sm get namespace help", &output);
        let stdout = stdout(&output);
        assert!(stdout.contains("--json"));
        assert!(!stdout.contains("--selector"));
        assert!(!stdout.contains("--namespace <NAMESPACE>"));
        assert!(!stdout.contains("--all-namespaces"));
    }
}

#[test]
pub(crate) fn create_help_lists_namespace_and_session_resources() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sm"))
        .args(["create", "--help"])
        .output()
        .or_panic("sm create help executes");

    assert_success("sm create --help", &output);
    let stdout = stdout(&output);
    assert!(stdout.contains("namespace"));
    assert!(stdout.contains("session"));
}

#[test]
pub(crate) fn create_session_help_exposes_only_declarative_arguments() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sm"))
        .args(["create", "session", "--help"])
        .output()
        .or_panic("sm create session help executes");

    assert_success("sm create session --help", &output);
    let stdout = stdout(&output);
    assert!(stdout.contains("<RUNTIME>"));
    assert!(stdout.contains("--role"));
    assert!(stdout.contains("--dir"));
    assert!(stdout.contains("--namespace"));
    assert!(stdout.contains("--label"));
    assert!(stdout.contains("--agent-config"));
    assert!(!stdout.contains("--isolation"));
    assert!(!stdout.contains("--image"));
    assert!(!stdout.contains("--target"));
    assert!(!stdout.contains("--detach"));
    assert!(!stdout.contains("--force"));
}

#[test]
pub(crate) fn run_help_exposes_force_as_imperative_argument() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sm"))
        .args(["run", "--help"])
        .output()
        .or_panic("sm run --help executes");

    assert_success("sm run --help", &output);
    let stdout = stdout(&output);
    assert!(stdout.contains("--force"));
    assert!(stdout.contains("Preempt an occupied tmux pane"));
}
