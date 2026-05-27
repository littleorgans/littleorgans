#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{
    RtmHarness, output_stdout, persist_running, unused_pid, wait_for_events,
    wait_for_status_timeout,
};
use uuid::Uuid;

#[test]
fn pass7_periodic_reconciliation_marks_lost_and_doctor_reports_it() {
    let harness = RtmHarness::start_with_fast_periodic_probe();
    let session_id = Uuid::now_v7();
    let runtime_pid = unused_pid();
    persist_running(harness.db_path(), session_id, runtime_pid);

    let status = wait_for_status_timeout(
        &harness,
        &session_id.to_string(),
        "state=Lost(PidNotAlive)",
        Duration::from_secs(3),
    );
    assert!(status.contains("runtime=claude"), "{status}");

    let events = wait_for_events(&harness, 1);
    assert!(events.contains("runtime event=Lost"), "{events}");
    assert!(events.contains(&session_id.to_string()), "{events}");
    assert!(events.contains("evidence=PidNotAlive"), "{events}");

    let doctor = harness.doctor();
    assert!(doctor.status.success(), "doctor failed: {doctor:?}");
    let doctor = output_stdout(doctor);
    assert!(doctor.contains("rtmd"), "{doctor}");
    assert!(doctor.contains("sqlite"), "{doctor}");
    assert!(
        doctor.contains("applied migrations  1 of 1 (unified schema)"),
        "{doctor}"
    );
    assert!(doctor.contains("lifecycles"), "{doctor}");
    assert!(doctor.contains("lost                1"), "{doctor}");
    assert!(doctor.contains("last probe sweep"), "{doctor}");
    assert!(!doctor.contains("last probe sweep      never"), "{doctor}");
    assert!(doctor.contains("recent lost"), "{doctor}");
    assert!(doctor.contains(&session_id.to_string()), "{doctor}");
    assert!(doctor.contains("PidNotAlive"), "{doctor}");

    harness.stop();
}
