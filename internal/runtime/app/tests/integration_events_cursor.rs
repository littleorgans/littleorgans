#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use common::{
    RtmHarness, output_stderr, output_stdout, runtime_event_line_count, spawn_ok, status_pid,
    terminate_process, wait_until,
};
use lilo_rm_core::{
    CursorExpiredPayload, EventsRequest, RuntimeEvent, RuntimeResponse, RuntimeRpc, WatcherCounts,
};
use serde_json::json;
use uuid::Uuid;

const FIRST_SESSION: &str = "018f6e28-0000-7000-8000-000000000101";
const SECOND_SESSION: &str = "018f6e28-0000-7000-8000-000000000102";

#[test]
fn events_resume_after_daemon_restart_without_duplication() {
    let mut harness = RtmHarness::start();
    spawn_ok(&harness, FIRST_SESSION, "claude");
    let cursor = wait_for_rpc_events(&harness, None, 1).cursor();

    harness.stop_rtmd();
    harness.start_rtmd();
    spawn_ok(&harness, SECOND_SESSION, "claude");

    let resumed = wait_for_rpc_events_at_least(&harness, Some(cursor), 1);

    let RuntimeResponse::Events(payload) = resumed else {
        panic!("expected events response");
    };
    let events = payload.events;
    let second_session = Uuid::parse_str(SECOND_SESSION).expect("second session id");
    assert!(
        events.iter().any(|event| matches!(
            event,
            lilo_rm_core::RuntimeEvent::Running { session_id, .. } if *session_id == second_session
        )),
        "{events:?}"
    );
    harness.stop();
}

#[test]
fn cli_events_since_matches_rpc_cursor_filter() {
    let harness = RtmHarness::start();
    spawn_ok(&harness, FIRST_SESSION, "claude");
    let cursor = wait_for_rpc_events(&harness, None, 1).cursor();
    spawn_ok(&harness, SECOND_SESSION, "claude");
    let rpc = wait_for_rpc_events(&harness, Some(cursor), 1);

    let output = harness.events_since(cursor);
    assert!(output.status.success(), "events --since failed: {output:?}");
    let stdout = output_stdout(output);

    let RuntimeResponse::Events(payload) = rpc else {
        panic!("expected events response");
    };
    let events = payload.events;
    assert_eq!(events.len(), runtime_event_line_count(&stdout));
    assert!(stdout.contains(SECOND_SESSION), "{stdout}");
    assert!(!stdout.contains(FIRST_SESSION), "{stdout}");
    harness.stop();
}

#[test]
fn cli_events_json_includes_resume_cursor() {
    let harness = RtmHarness::start();
    spawn_ok(&harness, FIRST_SESSION, "claude");
    let cursor = wait_for_rpc_events(&harness, None, 1).cursor();

    let output = harness.cli(&["events", "--format", "json"]);

    assert!(output.status.success(), "events json failed: {output:?}");
    let body: serde_json::Value =
        serde_json::from_str(&output_stdout(output)).expect("events json");
    assert_eq!(body["cursor"], cursor);
    assert_eq!(body["events"].as_array().expect("events array").len(), 1);
    harness.stop();
}

#[test]
fn cli_events_human_appends_resume_cursor() {
    let harness = RtmHarness::start();
    spawn_ok(&harness, FIRST_SESSION, "claude");

    let output = harness.events_since(0);

    assert!(output.status.success(), "events human failed: {output:?}");
    let stdout = output_stdout(output);
    assert!(stdout.contains(FIRST_SESSION), "{stdout}");
    assert_eq!(stdout.lines().last(), Some("cursor: 1"));
    harness.stop();
}

#[test]
fn events_since_cursor_returns_terminal_lifecycle_event() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7();
    spawn_ok(&harness, &session_id.to_string(), "claude");
    let cursor = wait_for_rpc_events(&harness, None, 1).cursor();
    let runtime_pid = status_pid(&harness, &session_id.to_string(), "pid");

    terminate_process(runtime_pid, "KILL");

    let response = wait_for_rpc_events(&harness, Some(cursor), 1);
    let RuntimeResponse::Events(payload) = response else {
        panic!("expected events response");
    };
    let events = payload.events;
    assert!(matches!(
        &events[0],
        RuntimeEvent::Terminated {
            session_id: observed,
            signal: Some(9),
            ..
        } if observed == &session_id
    ));
    harness.stop();
}

#[test]
fn expired_cursor_returns_cursor_expired_frame() {
    let mut harness = RtmHarness::start();
    harness.stop_rtmd();
    write_event_log(&harness, &[event_record(3, FIRST_SESSION)], "");
    harness.start_rtmd();

    let response = rpc_events(&harness, Some(0));

    assert_eq!(
        response,
        RuntimeResponse::CursorExpired(CursorExpiredPayload { oldest: 2 })
    );
    harness.stop();
}

#[test]
fn cli_events_json_surfaces_cursor_expiration() {
    let mut harness = RtmHarness::start();
    harness.stop_rtmd();
    write_event_log(&harness, &[event_record(3, FIRST_SESSION)], "");
    harness.start_rtmd();

    let output = harness.cli(&["events", "--since", "0", "--format", "json"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let body: serde_json::Value =
        serde_json::from_str(&output_stdout(output)).expect("cursor expired json");
    assert_eq!(body, json!({ "cursor_expired": true, "latest_cursor": 2 }));
    harness.stop();
}

#[test]
fn cli_events_human_surfaces_cursor_expiration_with_distinct_exit() {
    let mut harness = RtmHarness::start();
    harness.stop_rtmd();
    write_event_log(&harness, &[event_record(3, FIRST_SESSION)], "");
    harness.start_rtmd();

    let output = harness.events_since(0);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(
        output_stderr(output).trim(),
        "cursor expired (latest_cursor: 2)"
    );
    harness.stop();
}

#[test]
fn startup_recovery_drops_trailing_partial_event_line() {
    let mut harness = RtmHarness::start();
    harness.stop_rtmd();
    write_event_log(&harness, &[event_record(1, FIRST_SESSION)], r#"{"seq":2"#);
    harness.start_rtmd();

    let response = rpc_events(&harness, Some(0));

    let RuntimeResponse::Events(payload) = response else {
        panic!("expected events response");
    };
    let events = payload.events;
    let cursor = payload.cursor;
    assert_eq!(events.len(), 1);
    assert_eq!(cursor, 1);
    harness.stop();
}

#[test]
fn long_poll_times_out_with_unchanged_cursor() {
    let harness = RtmHarness::start();
    let start = Instant::now();

    let response = rpc_events_wait(&harness, Some(0), Some(500));
    let elapsed = start.elapsed();

    let RuntimeResponse::Events(payload) = response else {
        panic!("expected events response");
    };
    let events = payload.events;
    let cursor = payload.cursor;
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(cursor, 0);
    assert!(elapsed >= Duration::from_millis(450), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    harness.stop();
}

#[test]
fn long_poll_wakes_when_event_is_appended() {
    let harness = RtmHarness::start();
    let baseline = rpc_watchers(&harness).event_waiters;
    let socket_path = harness.socket_path().to_path_buf();
    let start = Instant::now();
    let waiter = thread::spawn(move || rpc_events_wait_path(socket_path, Some(0), Some(5_000)));

    wait_for_event_waiters_at_least(&harness, baseline + 1);
    spawn_ok(&harness, FIRST_SESSION, "claude");

    let response = waiter.join().expect("waiter");
    let RuntimeResponse::Events(payload) = response else {
        panic!("expected events response");
    };
    let events = payload.events;
    let cursor = payload.cursor;
    assert_eq!(events.len(), 1);
    assert_eq!(cursor, 1);
    assert!(start.elapsed() < Duration::from_secs(2));
    harness.stop();
}

#[test]
fn timed_out_long_poll_releases_waiter() {
    let harness = RtmHarness::start();
    let baseline = rpc_watchers(&harness).event_waiters;
    let socket_path = harness.socket_path().to_path_buf();
    let waiter = thread::spawn(move || rpc_events_wait_path(socket_path, Some(0), Some(100)));
    wait_for_event_waiters_at_least(&harness, baseline + 1);

    let response = waiter.join().expect("waiter");
    let RuntimeResponse::Events(payload) = response else {
        panic!("expected events response");
    };
    assert!(payload.events.is_empty());

    wait_for_event_waiters_at_most(&harness, baseline);
    harness.stop();
}

#[test]
fn concurrent_long_pollers_all_wake_on_single_append() {
    let harness = RtmHarness::start();
    let baseline = rpc_watchers(&harness).event_waiters;
    let socket_path = Arc::new(harness.socket_path().to_path_buf());
    let waiters: Vec<_> = (0..100)
        .map(|_| {
            let socket_path = Arc::clone(&socket_path);
            thread::spawn(move || rpc_events_wait_path(&*socket_path, Some(0), Some(5_000)))
        })
        .collect();

    wait_for_event_waiters_at_least(&harness, baseline + 100);
    spawn_ok(&harness, FIRST_SESSION, "claude");

    for waiter in waiters {
        let response = waiter.join().expect("waiter");
        let RuntimeResponse::Events(payload) = response else {
            panic!("expected events response");
        };
        let events = payload.events;
        let cursor = payload.cursor;
        assert_eq!(events.len(), 1);
        assert_eq!(cursor, 1);
    }
    harness.stop();
}

#[test]
fn cli_events_wait_ms_round_trips_through_json_scaffold() {
    let harness = RtmHarness::start();
    let start = Instant::now();

    let output = harness.events_wait_ms(0, 500);

    assert!(
        output.status.success(),
        "events --wait-ms failed: {output:?}"
    );
    let body: serde_json::Value =
        serde_json::from_str(&output_stdout(output)).expect("events json");
    assert_eq!(body, json!({ "events": [], "cursor": 0 }));
    assert!(start.elapsed() >= Duration::from_millis(450));
    harness.stop();
}

trait Cursor {
    fn cursor(&self) -> u64;
}

impl Cursor for RuntimeResponse {
    fn cursor(&self) -> u64 {
        match self {
            RuntimeResponse::Events(payload) => payload.cursor,
            other => panic!("expected events response, got {other:?}"),
        }
    }
}

fn wait_for_rpc_events(
    harness: &RtmHarness,
    since: Option<u64>,
    expected: usize,
) -> RuntimeResponse {
    wait_until(Duration::from_secs(5), || {
        let response = rpc_events(harness, since);
        match &response {
            RuntimeResponse::Events(payload) if payload.events.len() == expected => Some(response),
            _ => None,
        }
    })
    .unwrap_or_else(|| panic!("events never reached {expected}"))
}

fn wait_for_rpc_events_at_least(
    harness: &RtmHarness,
    since: Option<u64>,
    expected: usize,
) -> RuntimeResponse {
    wait_until(Duration::from_secs(5), || {
        let response = rpc_events(harness, since);
        match &response {
            RuntimeResponse::Events(payload) if payload.events.len() >= expected => Some(response),
            _ => None,
        }
    })
    .unwrap_or_else(|| panic!("events never reached {expected}"))
}

fn rpc_events(harness: &RtmHarness, since: Option<u64>) -> RuntimeResponse {
    rpc_events_wait(harness, since, None)
}

fn rpc_events_wait(
    harness: &RtmHarness,
    since: Option<u64>,
    wait_ms: Option<u32>,
) -> RuntimeResponse {
    rpc_events_wait_path(harness.socket_path(), since, wait_ms)
}

fn rpc_events_wait_path(
    socket_path: impl AsRef<std::path::Path>,
    since: Option<u64>,
    wait_ms: Option<u32>,
) -> RuntimeResponse {
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(lilo_rm_client::request(
            socket_path,
            RuntimeRpc::Events {
                request: EventsRequest { since, wait_ms },
            },
        ))
        .expect("events rpc")
}

fn rpc_watchers(harness: &RtmHarness) -> WatcherCounts {
    let response = tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(lilo_rm_client::request(
            harness.socket_path(),
            RuntimeRpc::Watchers,
        ))
        .expect("watchers rpc");
    let RuntimeResponse::Watchers(payload) = response else {
        panic!("expected watchers response");
    };
    payload.watchers
}

fn wait_for_event_waiters_at_least(harness: &RtmHarness, expected: usize) {
    wait_until(Duration::from_secs(5), || {
        (rpc_watchers(harness).event_waiters >= expected).then_some(())
    })
    .unwrap_or_else(|| panic!("event_waiters never reached at least {expected}"));
}

fn wait_for_event_waiters_at_most(harness: &RtmHarness, expected: usize) {
    wait_until(Duration::from_secs(5), || {
        (rpc_watchers(harness).event_waiters <= expected).then_some(())
    })
    .unwrap_or_else(|| panic!("event_waiters never returned to at most {expected}"));
}

fn write_event_log(harness: &RtmHarness, records: &[serde_json::Value], tail: &str) {
    let data_dir = harness.db_path().parent().expect("db parent");
    let path = lilo_paths::event_log_path(data_dir);
    create_dir_all(path.parent().expect("event log parent")).expect("event log dir");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)
        .expect("event log");
    for record in records {
        writeln!(file, "{record}").expect("record");
    }
    if !tail.is_empty() {
        file.write_all(tail.as_bytes()).expect("tail");
    }
}

fn event_record(seq: u64, session_id: &str) -> serde_json::Value {
    json!({
        "seq": seq,
        "ts_ms": 1_700_000_000_000_u64,
        "kind": "running",
        "payload": {
            "session_id": Uuid::parse_str(session_id).expect("session id"),
            "runtime_pid": 4242,
            "start_time": "2023-11-14T22:13:20Z"
        }
    })
}
