use std::fs::{self, OpenOptions};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use lilo_session_core::{DaemonStatus, RpcRequest, SmEndpoint, SmPaths};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::cli::cli_def::{DaemonAction, DaemonArgs};

pub async fn run(args: DaemonArgs) -> Result<()> {
    let paths = SmPaths::from_env()?;
    let endpoint = SmEndpoint::from_env()?;
    match args.action {
        DaemonAction::Start => start(&paths, &endpoint),
        DaemonAction::Stop => stop(&paths, &endpoint).await,
        DaemonAction::Status => {
            print_status(&status(&paths, &endpoint));
            Ok(())
        }
    }
}

fn start(paths: &SmPaths, endpoint: &SmEndpoint) -> Result<()> {
    fs::create_dir_all(&paths.dir).context("failed to create runtime directory")?;
    let current = status(paths, endpoint);
    if current.running {
        print_status(&current);
        return Ok(());
    }
    remove_stale_files(paths, endpoint);

    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log)
        .context("failed to open daemon log")?;
    let mut child = Command::new(std::env::current_exe().context("missing current executable")?)
        .arg("__smd")
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            log.try_clone().context("failed to clone daemon log")?,
        ))
        .stderr(Stdio::from(log))
        .spawn()
        .context("failed to spawn daemon")?;

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let current = status(paths, endpoint);
        if current.running {
            print_status(&current);
            return Ok(());
        }
        if let Some(exit) = child.try_wait().context("failed to observe daemon")? {
            bail!(
                "daemon exited before becoming ready: {exit}{}",
                daemon_log_tail(paths)
            );
        }
        thread::sleep(Duration::from_millis(100));
    }

    bail!("daemon did not become ready within 5s")
}

async fn stop(paths: &SmPaths, endpoint: &SmEndpoint) -> Result<()> {
    let current = status(paths, endpoint);
    if !current.running {
        print_status(&current);
        return Ok(());
    }

    if endpoint.exists() {
        let _ = lilo_session_daemon::send_request(endpoint, &RpcRequest::Shutdown).await;
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
    {
        bail!("daemon process {pid} did not stop");
    }

    remove_stale_files(paths, endpoint);
    print_status(&status(paths, endpoint));
    Ok(())
}

fn wait_for_stop(pid: Option<u32>, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pid.is_none_or(|pid| !process_alive(pid)) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn status(paths: &SmPaths, endpoint: &SmEndpoint) -> DaemonStatus {
    let pid = read_pid(paths);
    DaemonStatus {
        running: pid.is_some_and(process_alive) && endpoint.exists(),
        pid,
        pidfile: paths.pidfile.display().to_string(),
        endpoint: endpoint.to_string(),
    }
}

fn read_pid(paths: &SmPaths) -> Option<u32> {
    fs::read_to_string(&paths.pidfile)
        .ok()
        .and_then(|pid| pid.trim().parse().ok())
}

fn process_alive(pid: u32) -> bool {
    pid_from_u32(pid).is_some_and(|pid| kill(pid, None).is_ok())
}

fn signal_process(pid: u32, signal: Signal) {
    if let Some(pid) = pid_from_u32(pid) {
        let _ = kill(pid, signal);
    }
}

fn pid_from_u32(pid: u32) -> Option<Pid> {
    i32::try_from(pid).ok().map(Pid::from_raw)
}

fn remove_stale_files(paths: &SmPaths, endpoint: &SmEndpoint) {
    let _ = fs::remove_file(endpoint.as_path());
    let _ = fs::remove_file(&paths.pidfile);
}

fn daemon_log_tail(paths: &SmPaths) -> String {
    match fs::read_to_string(&paths.log) {
        Ok(contents) if !contents.trim().is_empty() => format!(": {}", contents.trim()),
        _ => String::new(),
    }
}

fn print_status(status: &DaemonStatus) {
    println!("{}", if status.running { "running" } else { "stopped" });
    if let Some(pid) = status.pid {
        println!("pid: {pid}");
    }
    println!("pidfile: {}", status.pidfile);
    println!("socket: {}", status.endpoint);
}
