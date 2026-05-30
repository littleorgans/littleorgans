use crate::common::{self, OrPanic as _};
use crate::{assert_success, assert_table_contains, first_field, stdout};
use serde_json::Value;

#[test]
pub(crate) fn removed_get_forms_are_rejected_by_clap() {
    for args in [
        ["get", "agent", "--help"].as_slice(),
        ["get", "agents", "--help"].as_slice(),
        ["get", "label", "--help"].as_slice(),
        ["get", "session", "--json"].as_slice(),
        ["get", "namespace", "--json"].as_slice(),
        ["get", "namespace", "--selector", "all"].as_slice(),
        ["get", "namespace", "namespace:default"].as_slice(),
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_sm"))
            .args(args)
            .output()
            .or_panic("sm get rejected form executes");

        assert!(!output.status.success());
    }
}

#[test]
pub(crate) fn session_resources_list_and_get_by_id() {
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
            "--label",
            "area=get",
        ])
        .output()
        .or_panic("sm run executes");
    assert_success("sm run", &run);
    let id = first_field(&run.stdout);

    let singular_list = daemon
        .command()
        .args(["get", "session"])
        .output()
        .or_panic("sm get session executes");
    assert_success("sm get session", &singular_list);
    assert_table_contains(&singular_list.stdout, &id);

    let plural_list = daemon
        .command()
        .args(["get", "sessions"])
        .output()
        .or_panic("sm get sessions executes");
    assert_success("sm get sessions", &plural_list);
    assert_table_contains(&plural_list.stdout, &id);

    let selected_list = daemon
        .command()
        .args(["get", "session", "--selector", "all"])
        .output()
        .or_panic("sm get session --selector all executes");
    assert_success("sm get session --selector all", &selected_list);
    assert_table_contains(&selected_list.stdout, &id);

    let labeled_list = daemon
        .command()
        .args(["get", "session", "--show-labels"])
        .output()
        .or_panic("sm get session --show-labels executes");
    assert_success("sm get session --show-labels", &labeled_list);
    let labeled_stdout = stdout(&labeled_list);
    assert!(labeled_stdout.starts_with("ID RUNTIME ROLE NAMESPACE DIR STATE PID TMUX LABELS"));
    assert!(labeled_stdout.contains("area=get"));

    let json_list = daemon
        .lilo_command()
        .args(["get", "session", "--output", "json"])
        .output()
        .or_panic("lilo get session --output json executes");
    assert_success("lilo get session --output json", &json_list);
    let sessions: Value = serde_json::from_slice(&json_list.stdout).or_panic("list JSON parses");
    assert!(sessions.as_array().is_some_and(|items| !items.is_empty()));

    let single = daemon
        .command()
        .args(["get", "session", &id])
        .output()
        .or_panic("sm get session <id> executes");
    assert_success("sm get session <id>", &single);
    let single_stdout = String::from_utf8_lossy(&single.stdout);
    assert!(single_stdout.contains(&id));
    assert!(!single_stdout.contains("area=get"));
    assert!(!single_stdout.starts_with("ID RUNTIME"));

    let labeled_single = daemon
        .command()
        .args(["get", "session", &id, "--show-labels"])
        .output()
        .or_panic("sm get session <id> --show-labels executes");
    assert_success("sm get session <id> --show-labels", &labeled_single);
    let labeled_single_stdout = stdout(&labeled_single);
    assert!(labeled_single_stdout.contains(&id));
    assert!(labeled_single_stdout.contains("area=get"));
}

#[test]
pub(crate) fn capture_takes_exact_session_id() {
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
        ])
        .output()
        .or_panic("sm run executes");
    assert_success("sm run", &run);
    let id = first_field(&run.stdout);

    let capture = daemon
        .lilo_command()
        .args([
            "capture",
            &id,
            "--output",
            "json",
            "--scrollback-lines",
            "20",
        ])
        .output()
        .or_panic("lilo capture <id> --output json executes");
    assert_success("lilo capture <id> --output json", &capture);
    let body: Value = serde_json::from_slice(&capture.stdout).or_panic("capture JSON parses");
    assert_eq!(body["session"]["id"], id);
    assert_eq!(body["capture"]["status"], "failed");

    let operator_capture = daemon
        .lilo_command()
        .args([
            "session",
            "capture",
            &id,
            "--output",
            "json",
            "--scrollback-lines",
            "20",
        ])
        .output()
        .or_panic("lilo session capture <id> --output json executes");
    assert_success("lilo session capture <id> --output json", &operator_capture);
    let body: Value =
        serde_json::from_slice(&operator_capture.stdout).or_panic("capture JSON parses");
    assert_eq!(body["session"]["id"], id);
    assert_eq!(body["capture"]["status"], "failed");

    for args in [
        ["capture", &id, "--json"].as_slice(),
        ["capture", "--selector", "all"].as_slice(),
        ["capture", "all"].as_slice(),
        ["capture", "role:engineer"].as_slice(),
        ["capture", "namespace:default"].as_slice(),
    ] {
        let output = daemon
            .command()
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("sm {} executes: {error}", args.join(" ")));
        assert!(
            !output.status.success(),
            "sm {} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
