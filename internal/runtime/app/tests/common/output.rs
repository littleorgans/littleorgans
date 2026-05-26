use std::process::Output;

use super::RtmHarness;

pub fn output_stdout(output: Output) -> String {
    String::from_utf8(output.stdout).expect("stdout")
}

pub fn output_stderr(output: Output) -> String {
    String::from_utf8(output.stderr).expect("stderr")
}

pub fn parse_runtime_pid(stdout: &str) -> u32 {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) {
        return json_u32(
            &value["payload"]["lifecycle"]["runtime_pid"],
            "runtime pid in spawn json",
        );
    }
    stdout
        .split("runtime_pid=")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .expect("runtime pid in spawn output")
}

pub fn parse_status_pid(stdout: &str) -> u32 {
    stdout.trim().parse().unwrap_or_else(|_| {
        let value: serde_json::Value = serde_json::from_str(stdout).expect("status json");
        json_u32(&value[0]["runtime_pid"], "status pid")
    })
}

pub fn spawn_ok(harness: &RtmHarness, session_id: &str, runtime: &str) -> String {
    let output = harness.spawn_runtime(session_id, runtime);
    spawn_output_ok(output, runtime)
}

pub fn spawn_output_ok(output: Output, runtime: &str) -> String {
    assert!(
        output.status.success(),
        "{runtime} spawn failed: {output:?}"
    );
    output_stdout(output)
}

pub fn status_pid(harness: &RtmHarness, session_id: &str, field: &str) -> u32 {
    let output = harness.status_format(session_id, "json");
    assert!(output.status.success(), "status failed: {output:?}");
    status_json_pid(&output_stdout(output), status_pid_field(field))
}

pub fn status_json_pid(stdout: &str, field: &str) -> u32 {
    let value: serde_json::Value = serde_json::from_str(stdout).expect("status json");
    json_u32(&value[0][field], "status pid field")
}

fn json_u32(value: &serde_json::Value, message: &str) -> u32 {
    u32::try_from(value.as_u64().expect(message)).expect(message)
}

fn status_pid_field(field: &str) -> &str {
    match field {
        "pid" => "runtime_pid",
        other => other,
    }
}

pub(super) fn parse_status_field(line: &str, key: &str) -> Option<u32> {
    line.split(&format!("{key}="))
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .and_then(|value| (value != "-").then_some(value))
        .and_then(|value| value.parse().ok())
}
