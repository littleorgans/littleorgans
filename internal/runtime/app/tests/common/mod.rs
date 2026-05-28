#![allow(dead_code, unused_imports)]

use std::fmt::Display;
use std::path::PathBuf;
use std::process::Command;

pub mod docker;
pub mod mcp;
pub mod tmux;

mod harness;
mod lifecycle;
mod output;
mod process;
mod wait;

pub use harness::{
    FAKE_RUNTIME_READY, RtmHarness, headless_spawn_request, headless_spawn_request_with_env,
};
pub use lifecycle::{persist_running, persist_running_with_start_time, unused_pid};
pub use output::{
    output_stderr, output_stdout, parse_runtime_pid, parse_status_pid, spawn_ok, spawn_output_ok,
    status_json_pid, status_pid,
};
pub use process::{assert_process_alive, process_alive, terminate_process, wait_for_child_exit};
pub use wait::{
    runtime_event_line_count, runtime_events_rpc, runtime_events_rpc_path, runtime_events_rpc_wait,
    runtime_watcher_counts, sigkill_runtime_and_wait_exited, sigkill_shim_then_runtime,
    wait_for_event_waiters_at_least, wait_for_event_waiters_at_most, wait_for_events,
    wait_for_events_since, wait_for_headless_runtime_ready, wait_for_json_status, wait_for_log,
    wait_for_log_contains, wait_for_rpc_events, wait_for_rpc_events_at_least, wait_for_status,
    wait_for_status_timeout, wait_until, wait_until_not_alive,
};

pub fn headless_spawn_command(session_id: impl Display) -> Command {
    let mut command = Command::new(workspace_bin("rtm"));
    command
        .arg("spawn")
        .arg("--runtime")
        .arg("claude")
        .arg("--session-id")
        .arg(session_id.to_string())
        .arg("--target")
        .arg("headless");
    command
}

pub fn bench_sample_count(default_samples: usize) -> usize {
    std::env::var("LILO_BENCH_SAMPLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_samples)
}

pub fn workspace_bin(name: &str) -> PathBuf {
    let override_env = format!("{}_TEST_BIN", name.to_ascii_uppercase().replace('-', "_"));
    if let Some(path) = std::env::var_os(override_env) {
        return PathBuf::from(path);
    }
    let cargo_env = format!("CARGO_BIN_EXE_{name}");
    if let Some(path) = std::env::var_os(cargo_env) {
        return PathBuf::from(path);
    }

    let current = std::env::current_exe().expect("current exe");
    let dir = current.parent().expect("executable parent");
    let candidate_dir = match dir.file_name().and_then(|name| name.to_str()) {
        Some("deps" | "examples") => dir.parent().expect("target profile dir"),
        _ => dir,
    };
    candidate_dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}
