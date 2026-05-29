#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::Value;

#[path = "../../../test_support.rs"]
mod shared_test_support;
#[allow(unused_imports)]
pub use lilo_test_support::{assert_success, stderr, stdout, write_fake_command};
pub use shared_test_support::OrPanic;

pub struct DaemonFixture {
    pub dir: tempfile::TempDir,
    daemon: lilo_test_support::LiloDaemon,
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
        let lilo_socket = dir.path().join("lilod.sock");
        let daemon = lilo_test_support::LiloDaemon::start(dir.path(), &lilo_socket, path_prefix)
            .or_panic("daemon starts");
        Self { dir, daemon }
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
        self.dir.path().join("data").join("lilo.db")
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
        self.daemon.socket_path().to_path_buf()
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(sm_bin());
        command
            .env_remove("CLAUDE_CONFIG_DIR")
            .env("LILO_HOME", self.dir.path())
            .env("LILO_SOCKET_PATH", self.daemon.socket_path())
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
        self.daemon.stop();
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
    if let Some(path) = std::env::var_os("LILO_BENCH_BIN") {
        return PathBuf::from(path);
    }
    assert_cmd::cargo::cargo_bin("sm")
}

pub fn create_namespace(daemon: &DaemonFixture, name: &str) {
    let output = daemon
        .command()
        .args(["create", "namespace", name])
        .output()
        .or_panic("sm create namespace executes");
    assert_success("sm create namespace", &output);
}

pub fn set_context(daemon: &DaemonFixture, name: &str) {
    let output = daemon
        .command()
        .args(["config", "set-context", name])
        .output()
        .or_panic("sm config set-context executes");
    assert_success("sm config set-context", &output);
}

pub fn namespace_binding_contents(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("config").join("session").join("namespace"))
        .or_panic("binding file reads")
}

pub fn first_table_field(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .split_whitespace()
        .next()
        .or_panic("stdout has first field")
        .to_string()
}

pub fn fake_runtime_path(command: &str) -> tempfile::TempDir {
    lilo_test_support::fake_runtime_path(command).or_panic("runtime path creates")
}
