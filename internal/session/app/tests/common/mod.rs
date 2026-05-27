#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

#[path = "../../../test_support.rs"]
mod shared_test_support;
pub use shared_test_support::OrPanic;

pub struct DaemonFixture {
    pub dir: tempfile::TempDir,
    child: Child,
    rtmd: Child,
    rtm: PathBuf,
    rtm_socket: PathBuf,
}

impl DaemonFixture {
    pub fn start() -> Self {
        Self::start_with_path_prefix(None)
    }

    pub fn start_with_runtime_path(path_prefix: &Path) -> Self {
        Self::start_with_path_prefix(Some(path_prefix))
    }

    fn start_with_path_prefix(path_prefix: Option<&Path>) -> Self {
        let dir = tempfile::tempdir().or_panic("tempdir creates");
        let rtm_socket = dir.path().join("rtm.sock");
        let rtm = rtm_bin();
        let mut rtmd = Command::new(&rtm)
            .arg("daemon")
            .arg("start")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env("RTM_SOCKET_PATH", &rtm_socket)
            .env("RTM_DB_PATH", dir.path().join("rtm.sqlite"))
            .env("RTM_HOME", dir.path().join("rtm-home"))
            .env("PATH", test_path(path_prefix))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .or_panic("rtmd starts");
        wait_for_path_socket(&rtm_socket, &mut rtmd);

        let mut command = Command::new(sm_bin());
        command
            .arg("__smd")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env("SM_HOME", dir.path())
            .env("HOME", dir.path())
            .env("RTM_SOCKET_PATH", &rtm_socket)
            .env("PATH", test_path(path_prefix));
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .or_panic("daemon starts");
        wait_for_socket(dir.path(), &mut child);
        Self {
            dir,
            child,
            rtmd,
            rtm,
            rtm_socket,
        }
    }

    pub fn spawn_mcp(&self) -> McpFixture {
        let child = self.mcp_command().spawn().or_panic("sm mcp starts");
        McpFixture {
            child,
            stdin: None,
            stdout: None,
        }
        .with_pipes()
    }

    pub fn spawn_mcp_for_session(&self, session_id: &str, current_dir: &Path) -> McpFixture {
        let child = self
            .mcp_command()
            .env("HELIOY_SESSION_ID", session_id)
            .current_dir(current_dir)
            .spawn()
            .or_panic("sm mcp starts");
        McpFixture {
            child,
            stdin: None,
            stdout: None,
        }
        .with_pipes()
    }

    pub fn audit_path(&self) -> PathBuf {
        self.dir.path().join("sm.db")
    }

    pub async fn audit_rows(&self) -> Vec<lilo_im_core::AuditRow> {
        let db = lilo_db::LiloDb::open_path(self.audit_path())
            .await
            .or_panic("audit db opens");
        lilo_im_store::query_audit(db.identity_pool(), lilo_im_store::AuditFilters::default())
            .await
            .or_panic("audit query succeeds")
    }

    pub fn socket_path(&self) -> PathBuf {
        self.dir.path().join("sock")
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(sm_bin());
        command
            .env_remove("CLAUDE_CONFIG_DIR")
            .env("SM_HOME", self.dir.path())
            .env("HOME", self.dir.path());
        command
    }

    fn mcp_command(&self) -> Command {
        let mut command = self.command();
        command
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        command
    }

    fn stop(&mut self) {
        let _ = self
            .command()
            .args(["daemon", "stop"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.child.wait();
        let _ = Command::new(&self.rtm)
            .args(["daemon", "stop"])
            .env("RTM_SOCKET_PATH", &self.rtm_socket)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.rtmd.kill();
        let _ = self.rtmd.wait();
    }
}

impl Drop for DaemonFixture {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct McpFixture {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl McpFixture {
    fn with_pipes(mut self) -> Self {
        self.stdin = Some(self.child.stdin.take().or_panic("mcp stdin"));
        self.stdout = Some(BufReader::new(
            self.child.stdout.take().or_panic("mcp stdout"),
        ));
        self
    }

    pub fn send(&mut self, request: &Value) -> Value {
        let line = serde_json::to_string(request).or_panic("request serializes");
        let stdin = self.stdin.as_mut().or_panic("mcp stdin open");
        writeln!(stdin, "{line}").or_panic("request writes");
        stdin.flush().or_panic("request flushes");

        let mut response = String::new();
        self.stdout
            .as_mut()
            .or_panic("mcp stdout open")
            .read_line(&mut response)
            .or_panic("response reads");
        serde_json::from_str(&response).or_panic("response parses")
    }
}

impl Drop for McpFixture {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

pub fn sm_bin() -> PathBuf {
    if let Some(path) = std::env::var_os("SM_BENCH_BIN") {
        return PathBuf::from(path);
    }
    assert_cmd::cargo::cargo_bin("sm")
}

fn wait_for_socket(dir: &Path, child: &mut Child) {
    let socket = dir.join("sock");
    wait_for_path_socket(&socket, child);
}

fn wait_for_path_socket(socket: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if socket.exists() {
            return;
        }
        if let Some(exit) = child.try_wait().or_panic("daemon can be observed") {
            panic!("daemon exited before socket became ready: {exit}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("daemon socket did not become ready");
}

const FAKE_RUNTIME_SCRIPT: &str = "#!/bin/sh\ntrap 'exit 0' TERM INT\nwhile :; do sleep 60; done\n";

pub fn fake_runtime_path(command: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().or_panic("runtime path tempdir creates");
    write_fake_command(dir.path(), command, FAKE_RUNTIME_SCRIPT);
    dir
}

pub fn write_fake_command(dir: &Path, command: &str, script: &str) {
    let runtime = dir.join(command);
    std::fs::write(&runtime, script).or_panic("fake command writes");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(&runtime)
            .or_panic("fake command metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&runtime, permissions).or_panic("fake command is executable");
    }
}

fn rtm_bin() -> PathBuf {
    if let Some(path) = std::env::var_os("RTM_TEST_BIN") {
        return PathBuf::from(path);
    }
    assert_cmd::cargo::cargo_bin("rtm")
}

fn test_path(prefix: Option<&Path>) -> std::ffi::OsString {
    let prefixes = prefix.into_iter().map(Path::to_path_buf);
    let paths = prefixes.chain(
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>()),
    );
    std::env::join_paths(paths).or_panic("PATH can be joined")
}
