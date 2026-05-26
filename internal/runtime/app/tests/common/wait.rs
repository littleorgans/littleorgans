use std::path::Path;
use std::time::{Duration, Instant};

use super::RtmHarness;
use super::harness::FAKE_RUNTIME_READY;
use super::output::output_stdout;
use super::process::process_alive;

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

pub fn runtime_event_line_count(stdout: &str) -> usize {
    stdout
        .lines()
        .filter(|line| line.starts_with("runtime event="))
        .count()
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

pub fn wait_until_not_alive(pid: u32) {
    wait_until(Duration::from_secs(5), || {
        (!process_alive(pid)).then_some(())
    })
    .unwrap_or_else(|| panic!("pid {pid} was still alive after SIGKILL"));
}
