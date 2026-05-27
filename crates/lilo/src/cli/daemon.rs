use std::fs;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use lilo_common::diagnostic::Diagnostic;
use lilo_paths::{LiloHome, LiloPaths, SmEndpoint};
use lilo_session_core::SessionRpc;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use serde::Serialize;

use super::Output;

#[derive(Debug, Parser)]
pub struct DaemonCli {
    #[command(subcommand)]
    action: DaemonAction,
}

#[derive(Debug, Subcommand)]
enum DaemonAction {
    Start,
    Stop,
    Status,
}

impl DaemonCli {
    pub async fn run(&self, output: Output) -> Result<(), Diagnostic> {
        match self.action {
            DaemonAction::Start => lilo_session_app::compose::run_from_env()
                .await
                .map_err(Diagnostic::from),
            DaemonAction::Stop => stop(&paths()?, output).await.map_err(Diagnostic::from),
            DaemonAction::Status => {
                print_status(output, &status(&paths()?));
                Ok(())
            }
        }
    }
}

async fn stop(paths: &LiloPaths, output: Output) -> Result<()> {
    let current = status(paths);
    if !current.running {
        print_status(output, &current);
        return Ok(());
    }

    if paths.socket_path().exists() {
        let endpoint = SmEndpoint::unix_socket(paths.socket_path());
        let _ = lilo_session_daemon::send_request(&endpoint, &SessionRpc::Shutdown).await;
    }

    wait_for_stop(current.pid, Duration::from_secs(5));
    if let Some(pid) = current.pid
        && process_alive(pid)
    {
        signal_process(pid, Signal::SIGTERM);
        wait_for_stop(Some(pid), Duration::from_millis(500));
    }
    if let Some(pid) = current.pid
        && process_alive(pid)
    {
        signal_process(pid, Signal::SIGKILL);
        wait_for_stop(Some(pid), Duration::from_millis(500));
    }
    if let Some(pid) = current.pid
        && process_alive(pid)
        && paths.socket_path().exists()
    {
        bail!("daemon process {pid} did not stop");
    }

    remove_stale_files(paths);
    print_status(output, &status(paths));
    Ok(())
}

fn status(paths: &LiloPaths) -> DaemonStatus {
    let pid = read_pid(paths);
    let running = pid.is_some_and(process_alive);
    DaemonStatus {
        pid,
        running,
        socket_exists: paths.socket_path().exists(),
    }
}

fn read_pid(paths: &LiloPaths) -> Option<u32> {
    fs::read_to_string(paths.pid_path())
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn process_alive(pid: u32) -> bool {
    pid_from_u32(pid).is_some_and(|pid| kill(pid, None).is_ok())
}

fn signal_process(pid: u32, signal: Signal) {
    if let Some(pid) = pid_from_u32(pid) {
        let _ = kill(pid, Some(signal));
    }
}

fn pid_from_u32(pid: u32) -> Option<Pid> {
    i32::try_from(pid).ok().map(Pid::from_raw)
}

fn wait_for_stop(pid: Option<u32>, timeout: Duration) {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if pid.is_none_or(|pid| !process_alive(pid)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn remove_stale_files(paths: &LiloPaths) {
    let _ = fs::remove_file(paths.socket_path());
    let _ = fs::remove_file(paths.pid_path());
}

fn paths() -> Result<LiloPaths> {
    let home = LiloHome::from_env()?;
    Ok(LiloPaths::new(home))
}

fn print_status(output: Output, status: &DaemonStatus) {
    match output {
        Output::Human => {
            if status.running {
                let pid = status.pid.unwrap_or_default();
                println!("lilod running pid={pid} socket={}", status.socket_exists);
            } else {
                println!("lilod not running socket={}", status.socket_exists);
            }
        }
        Output::Json => println!(
            "{}",
            serde_json::to_string(status).expect("daemon status serialization cannot fail")
        ),
    }
}

#[derive(Serialize)]
struct DaemonStatus {
    pid: Option<u32>,
    running: bool,
    socket_exists: bool,
}
