#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use common::{
    RtmHarness, output_stdout, runtime_event_line_count, status_pid, terminate_process,
    wait_for_headless_runtime_ready, wait_for_status, wait_for_status_timeout, wait_until,
    wait_until_not_alive,
};
use lilo_rm_core::StatusFilter;
use lilo_runtime_store::{LifecycleStore, StoreConfig};
use uuid::Uuid;

#[test]
fn pass4_restart_reconciles_sqlite_lifecycles() {
    let mut harness = RtmHarness::start();
    let sid1 = Uuid::now_v7().to_string();
    let sid2 = Uuid::now_v7().to_string();
    let sid3 = Uuid::now_v7().to_string();

    spawn(&harness, &sid1, "claude");
    spawn(&harness, &sid2, "claude");
    spawn(&harness, &sid3, "codex");
    wait_for_headless_runtime_ready(&harness, &sid1);
    wait_for_headless_runtime_ready(&harness, &sid2);
    wait_for_headless_runtime_ready(&harness, &sid3);
    let pid2 = status_pid(&harness, &sid2, "pid");

    harness.stop_rtmd();
    terminate_process(pid2, "KILL");
    wait_until_not_alive(pid2);
    harness.start_rtmd();

    let status1 = wait_for_status(&harness, &sid1, "state=Running");
    assert!(status1.contains("runtime=claude"), "{status1}");
    let status2 = wait_for_status_timeout(
        &harness,
        &sid2,
        "state=Lost(PidNotAlive)",
        Duration::from_secs(5),
    );
    assert!(status2.contains("runtime=claude"), "{status2}");
    let status3 = wait_for_status(&harness, &sid3, "state=Running");
    assert!(status3.contains("runtime=codex"), "{status3}");

    let events = wait_for_events_since(&harness, 3, 1);
    assert!(events.contains("runtime event=Lost"), "{events}");
    assert!(events.contains(&sid2), "{events}");
    assert!(events.contains("evidence=PidNotAlive"), "{events}");

    let states = persisted_states(harness.db_path());
    assert_eq!(states.len(), 3, "{states:?}");
    assert_eq!(states.get(&sid1).map(String::as_str), Some("Running"));
    assert_eq!(
        states.get(&sid2).map(String::as_str),
        Some("Lost(PidNotAlive)")
    );
    assert_eq!(states.get(&sid3).map(String::as_str), Some("Running"));
}

fn wait_for_events_since(harness: &RtmHarness, cursor: u64, expected: usize) -> String {
    wait_until(Duration::from_secs(5), || {
        let output = harness.events_since(cursor);
        let stdout = output_stdout(output);
        (runtime_event_line_count(&stdout) == expected).then_some(stdout)
    })
    .unwrap_or_else(|| panic!("events after cursor {cursor} never reached {expected}"))
}

fn spawn(harness: &RtmHarness, session_id: &str, runtime: &str) {
    let output = harness.spawn_runtime(session_id, runtime);
    assert!(
        output.status.success(),
        "{runtime} spawn failed: {output:?}"
    );
}

fn persisted_states(db_path: &Path) -> HashMap<String, String> {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let store = LifecycleStore::open(StoreConfig {
            db_path: db_path.to_path_buf(),
        })
        .await
        .expect("store");
        store
            .list(&StatusFilter::empty())
            .await
            .expect("lifecycles")
            .into_iter()
            .map(|lifecycle| {
                (
                    lifecycle.session_id.to_string(),
                    lifecycle.state.to_string(),
                )
            })
            .collect()
    })
}
