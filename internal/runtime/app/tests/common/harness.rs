use std::fmt::Write as _;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use lilo_common::id::SessionId;
use lilo_rm_core::{
    HeadlessSpawnTarget, IsolationPolicy, LaunchEnv, RuntimeKind, SpawnRequest, SpawnTarget,
};
use tempfile::TempDir;
use uuid::Uuid;

use super::output::{output_stdout, parse_status_field};
use super::process::{terminate_process, wait_for_child_exit};
use super::{docker, workspace_bin};

pub const FAKE_RUNTIME_READY: &str = "rtm fake runtime ready";

pub struct RtmHarness {
    temp: TempDir,
    socket: PathBuf,
    data_dir: PathBuf,
    testdb: Option<lilo_db::test_support::TestDb>,
    database_url: String,
    rtm_home: PathBuf,
    rtm: PathBuf,
    lilo: PathBuf,
    daemon: Child,
    reconcile_env: Vec<(&'static str, String)>,
    start_outside_tmux: bool,
}

impl RtmHarness {
    pub fn start() -> Self {
        Self::start_with_options(Vec::new(), false)
    }

    pub fn start_outside_tmux() -> Self {
        Self::start_with_options(Vec::new(), true)
    }

    pub fn start_with_tmux_server_label(server_label: &str) -> Self {
        Self::start_with_options(
            vec![("LILO_TMUX_SERVER_LABEL", server_label.to_owned())],
            false,
        )
    }

    pub fn start_with_docker_image(image: &str) -> Self {
        Self::start_with_options(vec![("LILO_DOCKER_IMAGE", image.to_owned())], true)
    }

    pub fn start_with_fast_resume_probe() -> Self {
        Self::start_with_options(
            vec![
                ("LILO_PROBE_SWEEP_INTERVAL_MS", "30000".to_owned()),
                ("LILO_RESUME_POLL_INTERVAL_MS", "25".to_owned()),
                ("LILO_RESUME_GAP_THRESHOLD_MS", "1".to_owned()),
            ],
            false,
        )
    }

    pub fn start_with_fast_periodic_probe() -> Self {
        Self::start_with_options(
            vec![("LILO_PROBE_SWEEP_INTERVAL_MS", "25".to_owned())],
            false,
        )
    }

    fn start_with_options(
        reconcile_env: Vec<(&'static str, String)>,
        start_outside_tmux: bool,
    ) -> Self {
        let temp = TempDir::new().expect("temp dir");
        let socket = temp.path().join("lilod.sock");
        let rtm_home = temp.path().join("lilo-home");
        let data_dir = rtm_home.join("data");
        write_fake_runtime(temp.path(), "claude");
        write_fake_runtime(temp.path(), "codex");
        docker::write_fake_cli(temp.path());
        let rtm = workspace_bin("rtm");
        let lilo = workspace_bin("lilo");
        // Provision a throwaway Postgres database and hand its URL to the daemon
        // subprocess via LILO_DATABASE_URL; the daemon resolves it through
        // open_postgres_resolved(). The fixture is kept alive for the harness
        // lifetime and dropped explicitly on stop()/Drop.
        let testdb =
            block_on(lilo_db::test_support::TestDb::create()).expect("provision test database");
        let database_url = testdb.database_url().to_owned();
        let mut daemon = start_daemon(
            &lilo,
            &socket,
            &rtm_home,
            temp.path(),
            &database_url,
            &reconcile_env,
            start_outside_tmux,
        );
        wait_for_socket(&socket, &mut daemon);
        Self {
            temp,
            socket,
            data_dir,
            testdb: Some(testdb),
            database_url,
            rtm_home,
            rtm,
            lilo,
            daemon,
            reconcile_env,
            start_outside_tmux,
        }
    }

    pub(super) fn temp_path(&self) -> &Path {
        self.temp.path()
    }

    pub fn spawn(&self, session_id: &str) -> Output {
        self.spawn_runtime(session_id, "claude")
    }

    pub fn spawn_runtime(&self, session_id: &str, runtime: &str) -> Output {
        self.spawn_command(session_id, runtime, "headless", true)
            .output()
            .expect("spawn client")
    }

    pub fn spawn_runtime_in_tmux(
        &self,
        session_id: &str,
        runtime: &str,
        tmux_address: &str,
    ) -> Output {
        self.spawn_command(session_id, runtime, &format!("tmux:{tmux_address}"), true)
            .output()
            .expect("spawn client")
    }

    pub fn kill(&self, session_id: &str, signal: &str, grace_secs: u64) -> Output {
        self.rtm_command()
            .arg("kill")
            .arg(session_id)
            .arg("--signal")
            .arg(signal)
            .arg("--grace-secs")
            .arg(grace_secs.to_string())
            .output()
            .expect("kill client")
    }

    pub fn status(&self, session_id: &str) -> Output {
        self.rtm_command()
            .arg("status")
            .arg("--session-id")
            .arg(session_id)
            .arg("--format")
            .arg("human")
            .output()
            .expect("status client")
    }

    pub fn status_format(&self, session_id: &str, format: &str) -> Output {
        self.rtm_command()
            .arg("status")
            .arg("--session-id")
            .arg(session_id)
            .arg("--format")
            .arg(format)
            .output()
            .expect("status client")
    }

    pub fn nudge(&self, session_id: &str, content: &str) -> Output {
        self.rtm_command()
            .arg("nudge")
            .arg(session_id)
            .arg("--content")
            .arg(content)
            .arg("--format")
            .arg("human")
            .output()
            .expect("nudge client")
    }

    pub fn events(&self) -> Output {
        self.rtm_command()
            .arg("events")
            .arg("--format")
            .arg("human")
            .output()
            .expect("events client")
    }

    pub fn events_since(&self, cursor: u64) -> Output {
        self.rtm_command()
            .arg("events")
            .arg("--since")
            .arg(cursor.to_string())
            .arg("--format")
            .arg("human")
            .output()
            .expect("events client")
    }

    pub fn events_wait_ms(&self, cursor: u64, wait_ms: u32) -> Output {
        self.rtm_command()
            .arg("events")
            .arg("--since")
            .arg(cursor.to_string())
            .arg("--wait-ms")
            .arg(wait_ms.to_string())
            .output()
            .expect("events client")
    }

    pub fn doctor(&self) -> Output {
        self.rtm_command()
            .arg("doctor")
            .arg("--format")
            .arg("human")
            .output()
            .expect("doctor client")
    }

    pub fn cli(&self, args: &[&str]) -> Output {
        self.rtm_command().args(args).output().expect("rtm client")
    }

    pub fn mcp_line(&self, line: &str) -> Output {
        let mut child = self
            .rtm_command()
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("mcp client");
        {
            let stdin = child.stdin.as_mut().expect("mcp stdin");
            stdin.write_all(line.as_bytes()).expect("write mcp line");
            stdin.write_all(b"\n").expect("write mcp newline");
        }
        child.wait_with_output().expect("mcp output")
    }

    pub fn start_rtmd(&mut self) {
        self.daemon = start_daemon(
            &self.lilo,
            &self.socket,
            &self.rtm_home,
            self.temp.path(),
            &self.database_url,
            &self.reconcile_env,
            self.start_outside_tmux,
        );
        wait_for_socket(&self.socket, &mut self.daemon);
    }

    pub fn stop_rtmd(&mut self) {
        let _ = self.daemon.kill();
        wait_for_child(&mut self.daemon);
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_file(self.rtm_home.join("run").join("lilod.pid"));
    }

    /// Postgres connection URL for the daemon's throwaway database. Tests that
    /// inspect persisted rows directly open a pool against this URL.
    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    /// Daemon data directory (`$LILO_HOME/data`); home of the event JSONL log.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    pub fn rtm_home(&self) -> &Path {
        &self.rtm_home
    }

    pub fn rtm_path(&self) -> &Path {
        &self.rtm
    }

    pub fn daemon_pid(&self) -> u32 {
        self.daemon.id()
    }

    pub fn stop(mut self) {
        self.cleanup_processes();
        stop_daemon(&self.lilo, &self.socket, &self.rtm_home, &mut self.daemon);
        if let Some(testdb) = self.testdb.take() {
            block_on(testdb.cleanup()).expect("test db cleans up");
        }
    }

    pub fn rtm_command(&self) -> Command {
        let mut command = Command::new(&self.rtm);
        command
            .env("LILO_SOCKET_PATH", &self.socket)
            .env("LILO_HOME", &self.rtm_home);
        command
    }

    pub fn spawn_command(
        &self,
        session_id: &str,
        runtime: &str,
        target: &str,
        human: bool,
    ) -> Command {
        let mut command = self.rtm_command();
        command
            .arg("spawn")
            .arg("--runtime")
            .arg(runtime)
            .arg("--session-id")
            .arg(session_id)
            .arg("--target")
            .arg(target);
        if human {
            command.arg("--format").arg("human");
        }
        command
    }

    fn cleanup_processes(&self) {
        let output = self
            .rtm_command()
            .arg("status")
            .arg("--format")
            .arg("human")
            .output();
        let Ok(output) = output else {
            return;
        };
        let stdout = output_stdout(output);
        for line in stdout.lines() {
            for key in ["runtime_pid", "shim_pid"] {
                if let Some(pid) = parse_status_field(line, key) {
                    terminate_process(pid, "KILL");
                }
            }
        }
    }
}

impl Drop for RtmHarness {
    fn drop(&mut self) {
        self.cleanup_processes();
        let _ = Command::new(&self.lilo)
            .arg("daemon")
            .arg("stop")
            .env("LILO_SOCKET_PATH", &self.socket)
            .env("LILO_HOME", &self.rtm_home)
            .output();
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        // Drop the throwaway database if stop() was not called (e.g. a panicking
        // test). Best-effort: TestDb::Drop warns about a leak on failure.
        if let Some(testdb) = self.testdb.take() {
            let _ = block_on(testdb.cleanup());
        }
    }
}

fn write_fake_runtime(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nif [ \"${{LILO_TEST_STDIO_SENTINELS:-}}\" = 1 ]; then\n  printf 'HELLO\\n'\n  printf 'WORLD\\n' >&2\n  exec sleep 60\nfi\nif [ \"${{LILO_TEST_TUI_EXIT_WINDOW:-}}\" = 1 ]; then\n  trap 'trap \"\" INT; printf \"press CTRL+C to quit\\n\"; sleep 1; exit 130' INT\n  printf '{FAKE_RUNTIME_READY}\\n'\n  while :; do sleep 60; done\nfi\nif [ -f .lilo-print-cwd ]; then\n  printf '{FAKE_RUNTIME_READY} %s\\n' \"$(pwd)\"\n  exec sleep 60\nfi\nif [ \"${{LILO_TEST_PRINT_ENV:-}}\" = 1 ]; then\n  env | sort\n  exec sleep 60\nfi\nprintf '{FAKE_RUNTIME_READY}\\n'\nexec sleep 60\n"
        ),
    )
    .expect("fake runtime");
    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("permissions");
    path
}

/// Drive `future` to completion from the sync harness, whether or not the
/// caller is already inside a tokio runtime. Runs on a dedicated OS thread with
/// its own current-thread runtime so it works from a sync `#[test]` and from an
/// async `#[tokio::test]` without nesting runtimes.
fn block_on<F>(future: F) -> F::Output
where
    F: std::future::Future + Send,
    F::Output: Send,
{
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime")
                    .block_on(future)
            })
            .join()
            .expect("harness runtime thread joins")
    })
}

fn start_daemon(
    lilo: &Path,
    socket: &Path,
    lilo_home: &Path,
    fake_bin_dir: &Path,
    database_url: &str,
    reconcile_env: &[(&'static str, String)],
    start_outside_tmux: bool,
) -> Child {
    let mut command = Command::new(lilo);
    command
        .arg("daemon")
        .arg("start")
        .env("LILO_SOCKET_PATH", socket)
        .env("LILO_HOME", lilo_home)
        .env("LILO_DATABASE_URL", database_url)
        .env("PATH", test_path(fake_bin_dir))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if start_outside_tmux {
        command.env_remove("TMUX").env_remove("TMUX_PANE");
    }
    for (key, value) in reconcile_env {
        command.env(key, value);
    }
    command.spawn().expect("daemon start")
}

fn test_path(fake_bin_dir: &Path) -> String {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let paths = std::iter::once(fake_bin_dir.to_path_buf()).chain(std::env::split_paths(&current));
    std::env::join_paths(paths)
        .expect("joined path")
        .to_string_lossy()
        .into_owned()
}

fn wait_for_socket(socket: &Path, daemon: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut last_error = None;
    while Instant::now() < deadline {
        match UnixStream::connect(socket) {
            Ok(_) => return,
            Err(error) => last_error = Some(error),
        }
        assert!(
            daemon.try_wait().expect("daemon try_wait").is_none(),
            "daemon exited before socket accepted connections at {}{}",
            socket.display(),
            daemon_debug(daemon)
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!(
        "daemon socket never accepted connections at {}; last error={last_error:?}",
        socket.display()
    );
}

fn stop_daemon(lilo: &Path, socket: &Path, lilo_home: &Path, daemon: &mut Child) {
    let output = Command::new(lilo)
        .arg("daemon")
        .arg("stop")
        .env("LILO_SOCKET_PATH", socket)
        .env("LILO_HOME", lilo_home)
        .output()
        .expect("daemon stop");
    assert!(
        output.status.success(),
        "stop failed: {output:?}{}",
        daemon_debug(daemon)
    );
    wait_for_child(daemon);
    assert!(!socket.exists(), "socket was not removed");
}

fn daemon_debug(daemon: &mut Child) -> String {
    let Ok(Some(status)) = daemon.try_wait() else {
        return String::new();
    };
    let mut debug = String::new();
    let _ = write!(debug, "; daemon exited with {status}");
    if let Some(stderr) = daemon.stderr.as_mut() {
        let mut contents = String::new();
        let _ = stderr.read_to_string(&mut contents);
        let _ = write!(debug, "; daemon stderr: {contents}");
    }
    debug
}

fn wait_for_child(child: &mut Child) {
    if wait_for_child_exit(child, Duration::from_secs(5)).is_some() {
        return;
    }
    let _ = child.kill();
    panic!("daemon did not exit");
}

pub fn headless_spawn_request(session_id: SessionId, cwd: impl Into<PathBuf>) -> SpawnRequest {
    headless_spawn_request_with_env(session_id, cwd, Vec::new())
}

pub fn headless_spawn_request_with_env(
    session_id: SessionId,
    cwd: impl Into<PathBuf>,
    env: Vec<LaunchEnv>,
) -> SpawnRequest {
    SpawnRequest {
        session_id,
        runtime: RuntimeKind::Claude,
        isolation: IsolationPolicy::default(),
        image: None,
        env,
        mounts: Vec::new(),
        cwd: cwd.into(),
        target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
        force: false,
        shell_resume: None,
        launch_attachment: None,
    }
}
