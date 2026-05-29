use std::path::Path;

use crate::common::{self, OrPanic as _};
pub(crate) use crate::common::{assert_success, stderr, stdout};
use serde_json::Value;

pub(crate) fn get_session_json(daemon: &common::DaemonFixture, id: &str) -> Value {
    let output = daemon
        .command()
        .args(["get", "session", id, "--json"])
        .output()
        .or_panic("sm get session <id> --json executes");
    assert_success("sm get session <id> --json", &output);
    serde_json::from_slice(&output.stdout).or_panic("session JSON parses")
}

pub(crate) fn assert_table_contains(stdout: &[u8], id: &str) {
    let stdout = String::from_utf8_lossy(stdout);
    assert!(stdout.starts_with("ID RUNTIME"));
    assert!(stdout.contains(id));
}

pub(crate) fn first_field(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .split_whitespace()
        .next()
        .or_panic("stdout has first field")
        .to_string()
}

pub(crate) fn canonical_display(path: &Path) -> Value {
    Value::String(
        std::fs::canonicalize(path)
            .or_panic("canonical path")
            .display()
            .to_string(),
    )
}
