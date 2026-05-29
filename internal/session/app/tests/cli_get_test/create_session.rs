use crate::common::{self, OrPanic as _};
use crate::{assert_success, canonical_display, first_field, get_session_json};
use serde_json::Value;

#[test]
pub(crate) fn create_session_persists_headless_record_without_foreground_attach() {
    let runtime_path = common::fake_runtime_path("claude");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());
    let project = daemon.dir.path().join("project");
    std::fs::create_dir_all(&project).or_panic("project dir");

    let created = daemon
        .command()
        .args([
            "create",
            "session",
            "claude",
            "--role",
            "engineer",
            "--dir",
            &project.display().to_string(),
            "--label",
            "area=create",
        ])
        .output()
        .or_panic("sm create session executes");
    assert_success("sm create session", &created);
    let id = first_field(&created.stdout);

    let session = get_session_json(&daemon, &id);
    let canonical_project = canonical_display(&project);
    assert_eq!(session["id"], id);
    assert_eq!(session["runtime"], "claude");
    assert_eq!(session["role"], "engineer");
    assert_eq!(session["namespace"], "default");
    assert_eq!(session["dir"], canonical_project);
    assert_eq!(session["workspace"], canonical_project);
    assert_eq!(session["state"], "RUNNING");
    assert_eq!(session["tmux_pane"], Value::Null);
    assert_eq!(session["labels"][0]["key"], "area");
    assert_eq!(session["labels"][0]["value"], "create");
}

#[test]
pub(crate) fn create_session_and_run_persist_compatible_records_for_shared_inputs() {
    let runtime_path = common::fake_runtime_path("claude");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());
    let project = daemon.dir.path().join("project");
    std::fs::create_dir_all(&project).or_panic("project dir");

    let run = daemon
        .command()
        .args([
            "run",
            "claude",
            "--role",
            "engineer",
            "--dir",
            &project.display().to_string(),
            "--label",
            "area=shared",
        ])
        .output()
        .or_panic("sm run executes");
    assert_success("sm run", &run);

    let created = daemon
        .command()
        .args([
            "create",
            "session",
            "claude",
            "--role",
            "engineer",
            "--dir",
            &project.display().to_string(),
            "--label",
            "area=shared",
        ])
        .output()
        .or_panic("sm create session executes");
    assert_success("sm create session", &created);

    let run_session = get_session_json(&daemon, &first_field(&run.stdout));
    let create_session = get_session_json(&daemon, &first_field(&created.stdout));
    for field in ["runtime", "role", "namespace", "dir", "workspace", "labels"] {
        assert_eq!(create_session[field], run_session[field], "{field} differs");
    }
    assert_eq!(create_session["tmux_pane"], run_session["tmux_pane"]);
    assert_eq!(create_session["agent_config"], run_session["agent_config"]);
}
