#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::process::Output;
use std::time::Duration;

use common::{
    RtmHarness, output_stdout, parse_runtime_pid, sigkill_runtime_and_wait_exited,
    sigkill_shim_then_runtime, spawn_ok, status_json_pid, wait_for_events, wait_for_status,
    wait_for_status_timeout,
};
use uuid::Uuid;

#[test]
fn kill_rpc_terminates_runtime_by_session_id() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    let spawn_stdout = spawn_ok(&harness, &session_id, "claude");
    let runtime_pid = parse_runtime_pid(&spawn_stdout);

    let json = output_stdout(harness.status_format(&session_id, "json"));
    assert!(json.contains(&session_id), "{json}");
    assert_eq!(status_json_pid(&json, "runtime_pid"), runtime_pid);

    let kill = harness.kill(&session_id, "TERM", 2);
    assert!(kill.status.success(), "kill failed: {kill:?}");
    let kill_stdout = output_stdout(kill);
    assert!(kill_stdout.contains("outcome"), "{kill_stdout}");
    assert!(kill_stdout.contains("signalled"), "{kill_stdout}");
    let status = wait_for_status_timeout(
        &harness,
        &session_id,
        "state=Exited",
        Duration::from_secs(1),
    );
    assert!(status.contains(&session_id), "{status}");

    let events = wait_for_events(&harness, 2);
    assert!(events.contains("runtime event=Running"), "{events}");
    assert!(events.contains("runtime event=Terminated"), "{events}");
    assert!(events.contains("evidence=ShimExit"), "{events}");

    let already_exited = kill_with_format(&harness, &session_id, "human");
    assert!(
        already_exited.status.success(),
        "already exited kill failed: {already_exited:?}"
    );
    let already_exited_stdout = output_stdout(already_exited);
    assert!(
        already_exited_stdout.contains("already exited"),
        "{already_exited_stdout}"
    );
}

#[test]
fn kill_rpc_reports_already_exited_as_json_success() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    spawn_ok(&harness, &session_id, "claude");

    sigkill_runtime_and_wait_exited(&harness, &session_id, Duration::from_secs(1));

    let kill = kill_with_format(&harness, &session_id, "json");
    assert!(kill.status.success(), "kill failed: {kill:?}");
    let stdout = output_stdout(kill);
    assert!(stdout.contains("\"outcome\""), "{stdout}");
    assert!(stdout.contains("\"already_exited\""), "{stdout}");
}

#[test]
fn direct_sigkill_runtime_is_reported_as_exited() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    spawn_ok(&harness, &session_id, "claude");

    let status = sigkill_runtime_and_wait_exited(&harness, &session_id, Duration::from_secs(1));
    assert!(status.contains("signal=9"), "{status}");
    let events = wait_for_events(&harness, 2);
    assert!(events.contains("runtime event=Terminated"), "{events}");
}

#[test]
fn process_exit_watcher_reports_lost_when_shim_dies_before_exit_report() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    spawn_ok(&harness, &session_id, "claude");

    sigkill_shim_then_runtime(&harness, &session_id);

    let status = wait_for_status(&harness, &session_id, "ShimDiedBeforeReport");
    assert!(
        status.contains("state=Lost") || status.contains("state=Exited"),
        "{status}"
    );
    let events = wait_for_events(&harness, 2);
    assert!(events.contains("runtime event=Lost"), "{events}");
}

fn kill_with_format(harness: &RtmHarness, session_id: &str, format: &str) -> Output {
    harness
        .rtm_command()
        .arg("kill")
        .arg(session_id)
        .arg("--format")
        .arg(format)
        .output()
        .expect("kill client")
}
