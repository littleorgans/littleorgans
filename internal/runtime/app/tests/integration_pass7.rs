#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{
    RtmHarness, output_stdout, persist_running, spawn_ok, unused_pid, wait_for_events,
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

#[test]
fn raw_runtime_spawn_does_not_write_session_tables() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7();

    spawn_ok(&harness, &session_id.to_string(), "claude");

    let counts = session_table_counts(harness.db_path(), session_id);
    assert_eq!(counts.spawn_intents, 0);
    assert_eq!(counts.sessions, 0);
    assert_eq!(counts.lifecycles, 1);

    harness.stop();
}

struct SessionTableCounts {
    spawn_intents: i64,
    sessions: i64,
    lifecycles: i64,
}

fn session_table_counts(path: &std::path::Path, session_id: Uuid) -> SessionTableCounts {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async move {
        let db = lilo_db::LiloDb::open_path(path).await.expect("open db");
        let id = session_id.to_string();
        SessionTableCounts {
            spawn_intents: count_rows(
                db.session_pool(),
                "SELECT COUNT(*) FROM session_spawn_intents WHERE session_id = ?",
                &id,
            )
            .await,
            sessions: count_rows(
                db.session_pool(),
                "SELECT COUNT(*) FROM session_sessions WHERE id = ?",
                &id,
            )
            .await,
            lifecycles: count_rows(
                db.runtime_pool(),
                "SELECT COUNT(*) FROM runtime_lifecycle WHERE session_id = ?",
                &id,
            )
            .await,
        }
    })
}

async fn count_rows(pool: &sqlx::SqlitePool, sql: &str, id: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count rows")
}
