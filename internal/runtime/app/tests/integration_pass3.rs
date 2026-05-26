#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use common::{RtmHarness, assert_process_alive, output_stderr, output_stdout, parse_runtime_pid};
use uuid::Uuid;

#[test]
fn pass3_spawn_dispatches_claude_and_codex_launchers() {
    let harness = RtmHarness::start();

    for runtime in ["claude", "codex"] {
        let session_id = Uuid::now_v7().to_string();
        let spawn = harness.spawn_runtime(&session_id, runtime);
        assert!(spawn.status.success(), "{runtime} spawn failed: {spawn:?}");

        let stdout = output_stdout(spawn);
        assert!(stdout.contains("spawn OK"), "{stdout}");
        let runtime_pid = parse_runtime_pid(&stdout);
        assert_process_alive(runtime_pid);

        let status = output_stdout(harness.status(&session_id));
        assert!(status.contains("state=Running"), "{status}");
        assert!(status.contains(&format!("runtime={runtime}")), "{status}");
    }

    harness.stop();
}

#[test]
fn pass3_unknown_runtime_returns_clean_registry_error() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    let spawn = harness.spawn_runtime(&session_id, "nonexistent");

    assert!(!spawn.status.success(), "unknown runtime spawn succeeded");
    let stderr = output_stderr(spawn);
    assert!(
        stderr.contains("no launcher registered for runtime kind: nonexistent"),
        "{stderr}"
    );

    harness.stop();
}
