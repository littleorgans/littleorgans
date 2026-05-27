use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Args;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_rm_core::{
    LaunchSpec, RuntimeExit, RuntimeSignal, ShellResume, ShimExit, ShimLaunchRequest, ShimReady,
};
use uuid::Uuid;

pub const SHIM_RECONNECT_MAX_ATTEMPTS: usize = 10;
const SHIM_RECONNECT_INITIAL_DELAY: Duration = Duration::from_secs(1);
const SHIM_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);
const RUNTIME_WAIT_POLL: Duration = Duration::from_millis(100);

static SIGTERM_RECEIVED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Args)]
pub struct ShimArgs {
    #[arg(long)]
    session_id: Uuid,
}

pub async fn run(args: ShimArgs) -> Result<()> {
    tokio::task::spawn_blocking(move || run_for_session_blocking(args.session_id))
        .await
        .context("shim task join failed")?
}

pub fn run_for_session_blocking(session_id: Uuid) -> Result<()> {
    ignore_user_interrupts()?;
    let socket_path = LiloPaths::new(LiloHome::from_env()?).socket_path();
    let launch_request = ShimLaunchRequest { session_id };
    let launch = reconnecting("ShimLaunch", || {
        lilo_runtime_daemon::shim_socket::request_launch_blocking(
            &socket_path,
            launch_request.clone(),
        )
    })?;
    let mut child = runtime_command(&launch)?
        .spawn()
        .context("failed to spawn runtime")?;
    let runtime_pid = child.id();

    let ready = ShimReady {
        session_id,
        shim_pid: std::process::id(),
        runtime_pid,
        start_time: lilo_runtime_platform::process::start_time_for_pid(runtime_pid)?
            .unwrap_or_else(chrono::Utc::now),
        tmux_pane: None,
    };
    reconnecting("ShimReady", || {
        lilo_runtime_daemon::shim_socket::send_ready_blocking(&socket_path, ready.clone())
    })?;

    let status = wait_for_runtime(&mut child)?;
    let exit = ShimExit {
        session_id,
        exit: runtime_exit(status),
    };
    reconnecting("ShimExit", || {
        lilo_runtime_daemon::shim_socket::send_exit_blocking(&socket_path, exit.clone())
    })?;
    if let Some(shell_resume) = launch.shell_resume.as_ref() {
        exec_shell_resume(shell_resume)?;
    }
    Ok(())
}

fn reconnecting<T, F>(label: &'static str, mut operation: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut delay = SHIM_RECONNECT_INITIAL_DELAY;
    for attempt in 1..=SHIM_RECONNECT_MAX_ATTEMPTS {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) if attempt == SHIM_RECONNECT_MAX_ATTEMPTS => {
                bail!("{label} failed after {SHIM_RECONNECT_MAX_ATTEMPTS} attempts: {error}");
            }
            Err(error) => {
                tracing::warn!(%error, attempt, label, "shim reconnect attempt failed");
                thread::sleep(delay);
                delay = std::cmp::min(delay * 2, SHIM_RECONNECT_MAX_DELAY);
            }
        }
    }
    bail!("{label} failed: reconnect loop exhausted without success or final failure")
}

fn runtime_command(launch: &LaunchSpec) -> Result<Command> {
    let mut command = Command::new(launch.command()?);
    command.args(launch.argv.iter().skip(1));
    apply_launch_env_cwd(&mut command, launch);
    restore_user_interrupts_before_exec(&mut command);
    Ok(command)
}

fn exec_shell_resume(resume: &ShellResume) -> Result<()> {
    let mut command = shell_resume_command(resume)?;
    let error = command.exec();
    Err(error).context("failed to exec shell after runtime exit")
}

fn shell_resume_command(resume: &ShellResume) -> Result<Command> {
    let mut command = Command::new(resume.command()?);
    command.args(resume.argv.iter().skip(1));
    command.env_clear();
    for env in &resume.env {
        command.env(&env.key, &env.value);
    }
    command.current_dir(&resume.cwd);
    restore_user_interrupts_before_exec(&mut command);
    Ok(command)
}

/// Apply `LaunchSpec.env` and `LaunchSpec.cwd` to a `Command`.
///
/// `env_clear()` is called first so the runtime starts from an empty env,
/// then `launch.env` is layered on top. Without this, the runtime would
/// inherit the shim's bootstrap env (`LILO_SOCKET_PATH`) and the daemon's
/// process env, defeating the denylist applied at capture time. `LaunchSpec.env`
/// is the authoritative source of truth for the runtime.
fn apply_launch_env_cwd(command: &mut Command, launch: &LaunchSpec) {
    command.env_clear();
    for env in &launch.env {
        command.env(&env.key, &env.value);
    }
    command.current_dir(&launch.cwd);
}

fn wait_for_runtime(child: &mut std::process::Child) -> Result<ExitStatus> {
    install_sigterm_handler()?;
    loop {
        if let Some(status) = child.try_wait().context("failed to poll runtime child")? {
            return Ok(status);
        }
        if SIGTERM_RECEIVED.swap(false, Ordering::SeqCst) {
            lilo_runtime_platform::signal::send_signal(child.id(), RuntimeSignal::Term)?;
            return child
                .wait()
                .context("failed to wait for runtime child after SIGTERM");
        }
        thread::sleep(RUNTIME_WAIT_POLL);
    }
}

fn install_sigterm_handler() -> Result<()> {
    // SAFETY: the handler only flips an atomic flag, which is async-signal-safe.
    let previous = unsafe {
        libc::signal(
            libc::SIGTERM,
            mark_sigterm as *const () as libc::sighandler_t,
        )
    };
    if previous == libc::SIG_ERR {
        return Err(std::io::Error::last_os_error()).context("failed to install SIGTERM handler");
    }
    Ok(())
}

fn ignore_user_interrupts() -> Result<()> {
    set_user_interrupt_disposition(libc::SIG_IGN)
}

fn restore_user_interrupts_before_exec(command: &mut Command) {
    // SAFETY: pre_exec runs in the child after fork and before exec. The closure
    // only resets signal dispositions through libc::signal.
    unsafe {
        command.pre_exec(|| {
            set_user_interrupt_disposition(libc::SIG_DFL).map_err(std::io::Error::other)
        });
    }
}

fn set_user_interrupt_disposition(handler: libc::sighandler_t) -> Result<()> {
    set_signal_disposition(libc::SIGINT, handler)?;
    set_signal_disposition(libc::SIGQUIT, handler)
}

fn set_signal_disposition(signal: libc::c_int, handler: libc::sighandler_t) -> Result<()> {
    // SAFETY: installing SIG_IGN or SIG_DFL does not capture Rust state.
    let previous = unsafe { libc::signal(signal, handler) };
    if previous == libc::SIG_ERR {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("failed to update signal disposition for {signal}"));
    }
    Ok(())
}

extern "C" fn mark_sigterm(_: libc::c_int) {
    SIGTERM_RECEIVED.store(true, Ordering::SeqCst);
}

fn runtime_exit(status: ExitStatus) -> RuntimeExit {
    RuntimeExit::new(status.code(), status.signal())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilo_rm_core::LaunchEnv;
    use std::path::PathBuf;

    #[test]
    fn apply_launch_env_cwd_clears_pre_existing_env_on_command() {
        // Pre-populate a Command with a sentinel env var to simulate inherited
        // env at the point apply_launch_env_cwd runs. The env_clear() inside
        // must wipe it before LaunchSpec.env is layered on top. Avoids mutating
        // the parent test process env (which is not single-thread safe under
        // Rust's default test harness).
        let launch = LaunchSpec {
            argv: vec!["/usr/bin/env".to_owned()],
            env: vec![LaunchEnv::new("RTM_ALLOWED_SENTINEL", "present")],
            cwd: PathBuf::from("/tmp"),
            shell_resume: None,
        };

        let mut command = Command::new("/usr/bin/env");
        command.env("RTM_PRE_EXISTING_SENTINEL", "should_be_cleared");
        apply_launch_env_cwd(&mut command, &launch);

        let output = command.output().expect("/usr/bin/env runs");
        assert!(output.status.success(), "env exited non-zero: {output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(
            stdout.contains("RTM_ALLOWED_SENTINEL=present"),
            "LaunchSpec.env was not delivered:\n{stdout}"
        );
        assert!(
            !stdout.contains("RTM_PRE_EXISTING_SENTINEL"),
            "pre-existing env was not cleared:\n{stdout}"
        );
        // The child should also not see PATH from this test process. Rust
        // defaults to inheriting unless env_clear is called, and we called it,
        // so the env map should be exactly LaunchSpec.env.
        assert!(
            !stdout.contains("PATH="),
            "env_clear should have prevented PATH inheritance:\n{stdout}"
        );
    }

    #[test]
    fn shell_resume_command_clears_pre_existing_env() {
        let resume = ShellResume {
            argv: vec!["/usr/bin/env".to_owned()],
            env: vec![LaunchEnv::new("SHELL_RESUME_SENTINEL", "present")],
            cwd: PathBuf::from("/tmp"),
        };

        let output = shell_resume_command(&resume)
            .expect("resume command")
            .output()
            .expect("/usr/bin/env runs");
        assert!(output.status.success(), "env exited non-zero: {output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(
            stdout.contains("SHELL_RESUME_SENTINEL=present"),
            "shell resume env was not delivered:\n{stdout}"
        );
        assert!(
            !stdout.contains("PATH="),
            "shell resume inherited caller env:\n{stdout}"
        );
    }
}
