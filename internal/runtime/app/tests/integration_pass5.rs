#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use common::{
    FAKE_RUNTIME_READY, RtmHarness, output_stderr, output_stdout, spawn_ok, spawn_output_ok,
    wait_for_status,
};
use lilo_rm_core::{CaptureError, CaptureRequest, CaptureResponse, RuntimeResponse, RuntimeRpc};
use std::{thread, time::Duration};
use uuid::Uuid;

#[test]
fn pass5_spawn_inside_tmux_captures_pane_and_nudges_it() {
    let Some(tmux_session) = common::tmux::TmuxSession::start("rtm-pass5") else {
        eprintln!("skipping tmux integration test because tmux is unavailable");
        return;
    };

    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    let expected_pane = tmux_session.pane();

    let spawn = harness.spawn_runtime_in_tmux(&session_id, "claude", &expected_pane);
    spawn_output_ok(spawn, "claude");
    let status = wait_for_json_status(&harness, &session_id, &expected_pane);
    assert!(
        status.contains(&format!("\"tmux_pane\": \"{expected_pane}\"")),
        "{status}"
    );
    tmux_session.assert_pane_listed(&expected_pane);
    tmux_session.wait_for_capture(FAKE_RUNTIME_READY);
    let capture = tmux_session.capture();
    assert!(!capture.contains("$ "), "{capture}");

    let content = format!("hello-from-rtm-{}", Uuid::now_v7().simple());
    let nudge = harness.nudge(&session_id, &content);
    assert!(nudge.status.success(), "nudge failed: {nudge:?}");
    let nudge_stdout = output_stdout(nudge);
    assert!(nudge_stdout.contains("nudge delivered"), "{nudge_stdout}");
    tmux_session.wait_for_capture(&content);

    harness.stop();
}

#[test]
fn nudge_rejects_terminal_tmux_session() {
    let Some(tmux_session) = common::tmux::TmuxSession::start("rtm-terminal-nudge") else {
        eprintln!("skipping tmux terminal nudge test because tmux is unavailable");
        return;
    };

    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    let expected_pane = tmux_session.pane();

    let spawn = harness.spawn_runtime_in_tmux(&session_id, "claude", &expected_pane);
    spawn_output_ok(spawn, "claude");
    tmux_session.wait_for_capture(FAKE_RUNTIME_READY);

    let kill = harness.kill(&session_id, "TERM", 2);
    assert!(kill.status.success(), "kill failed: {kill:?}");
    wait_for_status(&harness, &session_id, "state=Exited");

    let nudge = harness.nudge(&session_id, "after-exit");
    assert!(!nudge.status.success(), "nudge succeeded: {nudge:?}");
    let stderr = output_stderr(nudge);
    assert!(
        stderr.contains(&format!(
            "nudge failed; reason=session_ended session_id={session_id}"
        )),
        "{stderr}"
    );
    assert!(
        stderr.contains("detail=session is no longer running"),
        "{stderr}"
    );

    harness.stop();
}

#[test]
fn tmux_spawn_cwd_flag_overrides_caller_cwd() {
    let Some(tmux_session) = common::tmux::TmuxSession::start("rtm-tmux-cwd") else {
        eprintln!("skipping tmux cwd test because tmux is unavailable");
        return;
    };

    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    let caller_cwd = harness.rtm_home();
    std::fs::create_dir_all(caller_cwd).expect("caller cwd");
    let runtime_cwd = std::path::PathBuf::from(format!("/tmp/rtm-cwd-{}", Uuid::now_v7().simple()));
    std::fs::create_dir_all(&runtime_cwd).expect("runtime cwd");
    std::fs::write(runtime_cwd.join(".rtm-print-cwd"), "").expect("cwd marker");
    let runtime_cwd = std::fs::canonicalize(runtime_cwd).expect("canonical runtime cwd");
    let target = format!("tmux:{}", tmux_session.pane());

    let output = harness
        .spawn_command(&session_id, "claude", &target, true)
        .arg("--cwd")
        .arg(&runtime_cwd)
        .current_dir(caller_cwd)
        .output()
        .expect("spawn client");
    spawn_output_ok(output, "claude");

    tmux_session.wait_for_capture(&format!("{FAKE_RUNTIME_READY} {}", runtime_cwd.display()));
}

#[test]
fn capture_tmux_pane_returns_snapshot_json() {
    let Some(tmux_session) = common::tmux::TmuxSession::start("rtm-capture") else {
        eprintln!("skipping tmux integration test because tmux is unavailable");
        return;
    };

    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    let expected_pane = tmux_session.pane();
    spawn_output_ok(
        harness.spawn_runtime_in_tmux(&session_id, "claude", &expected_pane),
        "claude",
    );
    tmux_session.resize_height(5);
    tmux_session.wait_for_capture(FAKE_RUNTIME_READY);

    let marker = format!("capture-marker-{}", Uuid::now_v7().simple());
    let scrollback_payload = (0..80)
        .map(|line| format!("{marker}-{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let nudge = harness.nudge(&session_id, &scrollback_payload);
    assert!(nudge.status.success(), "nudge failed: {nudge:?}");
    tmux_session.wait_for_capture(&marker);

    let response = capture_rpc(&harness, session_id.parse().expect("session id"), Some(200));
    let RuntimeResponse::Capture(payload) = response else {
        panic!("unexpected capture response: {response:?}");
    };
    let snapshot = match payload.response {
        CaptureResponse::Captured(snapshot) => snapshot,
        other => panic!("unexpected capture payload: {other:?}"),
    };
    assert_eq!(snapshot.scrollback_lines_requested, 200);
    assert!(snapshot.content.contains(&marker), "{snapshot:?}");
    assert!(snapshot.scrollback_lines_included > 0, "{snapshot:?}");

    let output = harness.cli(&["capture", &session_id, "--scrollback-lines", "200"]);
    assert!(output.status.success(), "capture CLI failed: {output:?}");
    let snapshot: lilo_rm_core::PaneSnapshot =
        serde_json::from_str(&output_stdout(output)).expect("pane snapshot json");
    assert!(snapshot.content.contains(&marker), "{snapshot:?}");

    harness.stop();
}

#[test]
fn capture_headless_target_returns_not_tmux_target() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7();
    spawn_ok(&harness, &session_id.to_string(), "claude");

    let response = capture_rpc(&harness, session_id, None);
    let RuntimeResponse::Capture(payload) = response else {
        panic!("unexpected capture response: {response:?}");
    };
    match payload.response {
        CaptureResponse::Failed(CaptureError::NotATmuxTarget) => {}
        other => panic!("unexpected capture payload: {other:?}"),
    }

    harness.stop();
}

#[test]
fn capture_dead_tmux_pane_returns_pane_unavailable() {
    let Some(tmux_session) = common::tmux::TmuxSession::start("rtm-capture-dead") else {
        eprintln!("skipping tmux integration test because tmux is unavailable");
        return;
    };

    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7();
    let expected_pane = tmux_session.pane();
    spawn_output_ok(
        harness.spawn_runtime_in_tmux(&session_id.to_string(), "claude", &expected_pane),
        "claude",
    );
    tmux_session.wait_for_capture(FAKE_RUNTIME_READY);
    tmux_session.kill();

    let response = capture_rpc(&harness, session_id, None);
    let RuntimeResponse::Capture(payload) = response else {
        panic!("unexpected capture response: {response:?}");
    };
    match payload.response {
        CaptureResponse::Failed(CaptureError::PaneUnavailable) => {}
        other => panic!("unexpected capture payload: {other:?}"),
    }

    harness.stop();
}

#[test]
fn docker_tmux_pattern_a_container_survives_pane_close() {
    let Some(tmux_session) = common::tmux::TmuxSession::start("rtm-docker-pattern-a") else {
        eprintln!("skipping Docker tmux Pattern A test because tmux is unavailable");
        return;
    };

    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7();
    let expected_pane = tmux_session.pane();
    let spawn = harness
        .spawn_command(
            &session_id.to_string(),
            "claude",
            &format!("tmux:{expected_pane}"),
            true,
        )
        .arg("--isolation")
        .arg("docker")
        .arg("--image")
        .arg("runtime-matters-agent:latest")
        .output()
        .expect("spawn client");
    spawn_output_ok(spawn, "claude");
    let mut last_capture = String::new();
    common::wait_until(Duration::from_secs(5), || {
        last_capture = tmux_session.capture();
        last_capture.contains(FAKE_RUNTIME_READY).then_some(())
    })
    .unwrap_or_else(|| panic!("pane never contained Docker runtime ready: {last_capture}"));

    let container_pid = common::docker::container_pid(&harness, session_id);
    assert!(
        common::process_alive(container_pid),
        "fake container exited before pane close"
    );
    tmux_session.kill();
    thread::sleep(Duration::from_millis(300));
    assert!(
        common::process_alive(container_pid),
        "pane close should terminate attach without killing the container"
    );

    let kill = harness.kill(&session_id.to_string(), "TERM", 2);
    assert!(kill.status.success(), "kill failed: {kill:?}");
    common::wait_until_not_alive(container_pid);
    harness.stop();
}

#[test]
fn ctrl_c_induced_tmux_pane_loss_repro_covers_claude_and_codex() {
    for runtime in ["claude", "codex"] {
        ctrl_c_interrupts_runtime_without_losing_tmux_pane(runtime);
    }
}

fn ctrl_c_interrupts_runtime_without_losing_tmux_pane(runtime: &str) {
    let Some(tmux_session) = common::tmux::TmuxSession::start("rtm-ctrl-c-loss") else {
        eprintln!("skipping tmux Ctrl+C regression test because tmux is unavailable");
        return;
    };

    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    let expected_pane = tmux_session.pane();
    let spawn = harness
        .spawn_command(&session_id, runtime, &format!("tmux:{expected_pane}"), true)
        .env("RTM_TEST_TUI_EXIT_WINDOW", "1")
        .output()
        .expect("spawn client");
    spawn_output_ok(spawn, runtime);
    tmux_session.wait_for_capture(FAKE_RUNTIME_READY);

    assert!(
        tmux_session.send_ctrl_c(&expected_pane),
        "failed to send initial Ctrl+C to {expected_pane}"
    );
    tmux_session.wait_for_capture("press CTRL+C to quit");

    for _ in 0..7 {
        let _ = tmux_session.send_ctrl_c(&expected_pane);
        thread::sleep(Duration::from_millis(25));
    }

    let status = wait_for_interrupt_recovery(&harness, &tmux_session, &session_id, &expected_pane);
    eprintln!("ALP-2597 regression runtime={runtime} status={status}");
    assert!(!status.contains("ShimDiedBeforeReport"), "{status}");
    assert!(
        !status.contains("\"reason\": \"pane_unavailable\""),
        "{status}"
    );

    let next_session_id = Uuid::now_v7().to_string();
    let respawn = harness
        .spawn_command(
            &next_session_id,
            runtime,
            &format!("tmux:{expected_pane}"),
            true,
        )
        .output()
        .expect("respawn client");
    spawn_output_ok(respawn, runtime);
    tmux_session.wait_for_capture(FAKE_RUNTIME_READY);

    harness.stop();
}

fn wait_for_json_status(harness: &RtmHarness, session_id: &str, needle: &str) -> String {
    common::wait_until(std::time::Duration::from_secs(5), || {
        let output = harness.status_format(session_id, "json");
        let stdout = output_stdout(output);
        stdout.contains(needle).then_some(stdout)
    })
    .unwrap_or_else(|| panic!("json status never contained {needle}"))
}

fn wait_for_interrupt_recovery(
    harness: &RtmHarness,
    tmux_session: &common::tmux::TmuxSession,
    session_id: &str,
    expected_pane: &str,
) -> String {
    let mut last_status = String::new();
    let mut last_pane_alive = false;
    common::wait_until(Duration::from_secs(30), || {
        let output = harness.status_format(session_id, "json");
        let stdout = output_stdout(output);
        let exited = stdout.contains("\"exited\"");
        let pane_alive = tmux_session.pane_alive(expected_pane);
        last_status.clone_from(&stdout);
        last_pane_alive = pane_alive;
        (exited && pane_alive).then_some(stdout)
    })
    .unwrap_or_else(|| {
        panic!(
            "runtime did not exit with live tmux pane {expected_pane}; pane_alive={last_pane_alive}; last_status={last_status}"
        )
    })
}

fn capture_rpc(
    harness: &RtmHarness,
    session_id: Uuid,
    scrollback_lines: Option<u32>,
) -> RuntimeResponse {
    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(lilo_runtime_app::shared::request(
            harness.socket_path(),
            RuntimeRpc::Capture {
                request: CaptureRequest {
                    session_id,
                    scrollback_lines,
                },
            },
        ))
        .expect("capture rpc")
}
