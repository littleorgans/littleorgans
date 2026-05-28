use std::path::Path;
use std::time::{Duration, Instant};

use lilo_rm_core::{EventsRequest, RuntimeResponse, RuntimeRpc, WatcherCounts};

use super::RtmHarness;
use super::harness::FAKE_RUNTIME_READY;
use super::output::{output_stdout, status_pid};
use super::process::{process_alive, terminate_process};

pub fn wait_for_status(harness: &RtmHarness, session_id: &str, needle: &str) -> String {
    wait_for_status_timeout(harness, session_id, needle, Duration::from_secs(5))
}

pub fn wait_for_status_timeout(
    harness: &RtmHarness,
    session_id: &str,
    needle: &str,
    timeout: Duration,
) -> String {
    let mut last_status = String::new();
    wait_until(timeout, || {
        let output = harness.status(session_id);
        let success = output.status.success();
        let stdout = String::from_utf8(output.stdout).expect("stdout");
        let stderr = String::from_utf8(output.stderr).expect("stderr");
        last_status = format!("success={success} stdout={stdout:?} stderr={stderr:?}");
        stdout.contains(needle).then_some(stdout)
    })
    .unwrap_or_else(|| panic!("status never contained {needle}; last status: {last_status}"))
}

pub fn wait_for_events(harness: &RtmHarness, expected: usize) -> String {
    wait_until(Duration::from_secs(5), || {
        let output = harness.events();
        let stdout = output_stdout(output);
        (runtime_event_line_count(&stdout) == expected).then_some(stdout)
    })
    .unwrap_or_else(|| panic!("events never reached {expected}"))
}

pub fn wait_for_events_since(harness: &RtmHarness, cursor: u64, expected: usize) -> String {
    wait_until(Duration::from_secs(5), || {
        let output = harness.events_since(cursor);
        let stdout = output_stdout(output);
        (runtime_event_line_count(&stdout) == expected).then_some(stdout)
    })
    .unwrap_or_else(|| panic!("events after cursor {cursor} never reached {expected}"))
}

pub fn runtime_event_line_count(stdout: &str) -> usize {
    stdout
        .lines()
        .filter(|line| line.starts_with("runtime event="))
        .count()
}

pub fn wait_for_json_status(harness: &RtmHarness, session_id: &str, needle: &str) -> String {
    wait_until(Duration::from_secs(5), || {
        let output = harness.status_format(session_id, "json");
        let stdout = output_stdout(output);
        stdout.contains(needle).then_some(stdout)
    })
    .unwrap_or_else(|| panic!("json status never contained {needle}"))
}

pub fn sigkill_runtime_and_wait_exited(
    harness: &RtmHarness,
    session_id: &str,
    timeout: Duration,
) -> String {
    let runtime_pid = status_pid(harness, session_id, "pid");
    terminate_process(runtime_pid, "KILL");
    wait_for_status_timeout(harness, session_id, "state=Exited", timeout)
}

pub fn sigkill_shim_then_runtime(harness: &RtmHarness, session_id: &str) {
    let shim_pid = status_pid(harness, session_id, "shim_pid");
    let runtime_pid = status_pid(harness, session_id, "pid");
    terminate_process(shim_pid, "KILL");
    terminate_process(runtime_pid, "KILL");
}

pub fn runtime_events_rpc(harness: &RtmHarness, since: Option<u64>) -> RuntimeResponse {
    runtime_events_rpc_wait(harness, since, None)
}

pub fn runtime_events_rpc_wait(
    harness: &RtmHarness,
    since: Option<u64>,
    wait_ms: Option<u32>,
) -> RuntimeResponse {
    runtime_events_rpc_path(harness.socket_path(), since, wait_ms)
}

pub fn runtime_events_rpc_path(
    socket_path: impl AsRef<Path>,
    since: Option<u64>,
    wait_ms: Option<u32>,
) -> RuntimeResponse {
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(lilo_runtime_app::shared::request(
            socket_path.as_ref(),
            RuntimeRpc::Events {
                request: EventsRequest { since, wait_ms },
            },
        ))
        .expect("events rpc")
}

pub fn wait_for_rpc_events(
    harness: &RtmHarness,
    since: Option<u64>,
    expected: usize,
) -> RuntimeResponse {
    wait_for_rpc_events_matching(harness, since, expected, |actual| actual == expected)
}

pub fn wait_for_rpc_events_at_least(
    harness: &RtmHarness,
    since: Option<u64>,
    expected: usize,
) -> RuntimeResponse {
    wait_for_rpc_events_matching(harness, since, expected, |actual| actual >= expected)
}

fn wait_for_rpc_events_matching(
    harness: &RtmHarness,
    since: Option<u64>,
    expected: usize,
    is_match: impl Fn(usize) -> bool,
) -> RuntimeResponse {
    wait_until(Duration::from_secs(5), || {
        let response = runtime_events_rpc(harness, since);
        match &response {
            RuntimeResponse::Events(payload) if is_match(payload.events.len()) => Some(response),
            _ => None,
        }
    })
    .unwrap_or_else(|| panic!("events never reached {expected}"))
}

pub fn runtime_watcher_counts(harness: &RtmHarness) -> WatcherCounts {
    let response = tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(lilo_runtime_app::shared::request(
            harness.socket_path(),
            RuntimeRpc::Watchers,
        ))
        .expect("watchers rpc");
    let RuntimeResponse::Watchers(payload) = response else {
        panic!("expected watchers response");
    };
    payload.watchers
}

pub fn wait_for_event_waiters_at_least(harness: &RtmHarness, expected: usize) {
    wait_until(Duration::from_secs(5), || {
        (runtime_watcher_counts(harness).event_waiters >= expected).then_some(())
    })
    .unwrap_or_else(|| panic!("event_waiters never reached at least {expected}"));
}

pub fn wait_for_event_waiters_at_most(harness: &RtmHarness, expected: usize) {
    wait_until(Duration::from_secs(5), || {
        (runtime_watcher_counts(harness).event_waiters <= expected).then_some(())
    })
    .unwrap_or_else(|| panic!("event_waiters never returned to at most {expected}"));
}

pub fn wait_until<T>(timeout: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(value) = check() {
            return Some(value);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

pub fn wait_for_headless_runtime_ready(harness: &RtmHarness, session_id: &str) {
    wait_for_log(
        harness
            .rtm_home()
            .join("logs")
            .join("runtimes")
            .join(session_id)
            .join("stdout.log"),
        &format!("{FAKE_RUNTIME_READY}\n"),
    );
}

pub fn wait_for_log(path: impl AsRef<Path>, expected: &str) {
    let path = path.as_ref();
    if wait_until(Duration::from_secs(5), || {
        std::fs::read_to_string(path)
            .ok()
            .filter(|contents| contents == expected)
    })
    .is_none()
    {
        let observed = std::fs::read_to_string(path);
        panic!(
            "log {} expected {expected:?}, observed {observed:?}",
            path.display()
        );
    }
}

pub fn wait_for_log_contains(path: impl AsRef<Path>, expected: &str) -> String {
    let path = path.as_ref();
    wait_until(Duration::from_secs(5), || {
        let contents = std::fs::read_to_string(path).ok()?;
        contents.contains(expected).then_some(contents)
    })
    .unwrap_or_else(|| {
        let observed = std::fs::read_to_string(path);
        panic!(
            "log {} never contained {expected:?}, observed {observed:?}",
            path.display()
        )
    })
}

pub fn wait_until_not_alive(pid: u32) {
    wait_until(Duration::from_secs(5), || {
        (!process_alive(pid)).then_some(())
    })
    .unwrap_or_else(|| panic!("pid {pid} was still alive after SIGKILL"));
}
