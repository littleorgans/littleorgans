#![allow(dead_code, unused_imports)]

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
