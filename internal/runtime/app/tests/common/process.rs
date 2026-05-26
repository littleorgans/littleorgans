use std::process::{Command, Stdio};

pub fn assert_process_alive(pid: u32) {
    let status = Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .status()
        .expect("ps");
    assert!(status.success(), "runtime pid {pid} is not alive");
}

pub fn terminate_process(pid: u32, signal: &str) {
    let _ = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .stderr(Stdio::null())
        .status();
}

pub fn process_alive(pid: u32) -> bool {
    Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("ps")
        .success()
}
