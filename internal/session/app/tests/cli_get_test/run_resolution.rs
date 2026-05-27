use crate::common::{self, OrPanic as _};
use crate::{assert_success, canonical_display, first_field};
use serde_json::Value;

#[test]
pub(crate) fn run_persists_canonical_dir_from_cli_resolution() {
    let runtime_path = common::fake_runtime_path("claude");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());
    let project = daemon.dir.path().join("project");
    std::fs::create_dir_all(&project).or_panic("project dir");

    let run = daemon
        .command()
        .current_dir(&project)
        .args([
            "run", "claude", "--role", "engineer", "--dir", ".", "--detach",
        ])
        .output()
        .or_panic("sm run executes");
    assert_success("sm run --dir", &run);
    let id = first_field(&run.stdout);

    let single = daemon
        .command()
        .args(["get", "session", &id, "--json"])
        .output()
        .or_panic("sm get session <id> --json executes");
    assert_success("sm get session <id> --json", &single);
    let session: Value = serde_json::from_slice(&single.stdout).or_panic("session JSON parses");
    let canonical_project = canonical_display(&project);
    assert_eq!(session["dir"], canonical_project);
    assert_eq!(session["workspace"], canonical_project);
    assert_eq!(session["namespace"], "default");
}

#[test]
pub(crate) fn workspace_arg_is_rejected_by_clap() {
    let daemon = common::DaemonFixture::start();

    let run = daemon
        .command()
        .args([
            "run",
            "claude",
            "--role",
            "engineer",
            "--dir",
            &daemon.dir.path().display().to_string(),
            "--workspace",
            &daemon.dir.path().display().to_string(),
        ])
        .output()
        .or_panic("sm run executes");

    assert!(!run.status.success());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("unexpected argument '--workspace'"));
    assert!(!stderr.contains("--workspace is deprecated"));
}

#[test]
pub(crate) fn unknown_namespace_error_is_surfaced_from_daemon() {
    let runtime_path = common::fake_runtime_path("claude");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());

    let run = daemon
        .command()
        .args([
            "run",
            "claude",
            "--role",
            "engineer",
            "--dir",
            &daemon.dir.path().display().to_string(),
            "--namespace",
            "missing",
            "--detach",
        ])
        .output()
        .or_panic("sm run executes");

    assert!(!run.status.success());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("namespace not found: missing"));
}
