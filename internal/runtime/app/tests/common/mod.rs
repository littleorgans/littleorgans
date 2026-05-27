#![allow(dead_code, unused_imports)]

use std::path::PathBuf;

pub mod docker;
pub mod mcp;
pub mod tmux;

mod harness;
mod lifecycle;
mod output;
mod process;
mod wait;

pub use harness::{FAKE_RUNTIME_READY, RtmHarness};
pub use lifecycle::{persist_running, persist_running_with_start_time, unused_pid};
pub use output::{
    output_stderr, output_stdout, parse_runtime_pid, parse_status_pid, spawn_ok, spawn_output_ok,
    status_json_pid, status_pid,
};
pub use process::{assert_process_alive, process_alive, terminate_process};
pub use wait::{
    runtime_event_line_count, wait_for_events, wait_for_headless_runtime_ready, wait_for_log,
    wait_for_status, wait_for_status_timeout, wait_until, wait_until_not_alive,
};

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
