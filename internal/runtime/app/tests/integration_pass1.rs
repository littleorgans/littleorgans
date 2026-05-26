#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use common::{RtmHarness, assert_process_alive, output_stdout, parse_runtime_pid, wait_for_events};
use uuid::Uuid;

#[test]
fn pass1_spawn_records_running_lifecycle_and_event() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    let spawn = harness.spawn(&session_id);
    assert!(spawn.status.success(), "spawn failed: {spawn:?}");

    let stdout = output_stdout(spawn);
    assert!(stdout.contains("spawn OK"));
    assert!(stdout.contains("lifecycle state=Running"));
    assert!(stdout.contains("runtime event=Running"));
    let log_dir = harness.rtm_home().join("logs").join(&session_id);
    assert!(
        stdout.contains(&format!("log_dir={}", log_dir.display())),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "stdout_path={}",
            log_dir.join("stdout.log").display()
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "stderr_path={}",
            log_dir.join("stderr.log").display()
        )),
        "{stdout}"
    );

    let runtime_pid = parse_runtime_pid(&stdout);
    assert_process_alive(runtime_pid);

    let status = output_stdout(harness.status(&session_id));
    assert!(status.contains("state=Running"), "{status}");
    assert!(status.contains(&session_id), "{status}");

    let events = wait_for_events(&harness, 1);
    assert!(events.contains("runtime event=Running"), "{events}");
    assert!(events.contains(&session_id), "{events}");

    harness.stop();
}
