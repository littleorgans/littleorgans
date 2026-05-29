use std::ffi::{OsStr, OsString};
use std::io::{ErrorKind, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use tempfile::TempDir;

pub const DAEMON_TIMEOUT: Duration = Duration::from_secs(10);
pub const FAKE_RUNTIME_SCRIPT: &str =
    "#!/bin/sh\ntrap 'exit 0' TERM INT\nwhile :; do sleep 60; done\n";

pub struct LiloDaemon {
    child: Option<Child>,
    lilo: PathBuf,
    home: PathBuf,
    socket: PathBuf,
    path: OsString,
    timeout: Duration,
}

impl LiloDaemon {
    pub fn start(
        home: impl AsRef<Path>,
        socket: impl AsRef<Path>,
        path_prefix: Option<&Path>,
    ) -> Result<Self> {
        Self::start_with_timeout(home, socket, path_prefix, DAEMON_TIMEOUT)
    }

    pub fn start_with_timeout(
        home: impl AsRef<Path>,
        socket: impl AsRef<Path>,
        path_prefix: Option<&Path>,
        timeout: Duration,
    ) -> Result<Self> {
        let lilo = lilo_bin();
        let home = home.as_ref().to_path_buf();
        let socket = socket.as_ref().to_path_buf();
        let path = test_path(path_prefix)?;
        let mut child = Command::new(&lilo)
            .args(["daemon", "start"])
            .env_remove("CLAUDE_CONFIG_DIR")
            .env("LILO_HOME", &home)
            .env("LILO_SOCKET_PATH", &socket)
            .env("HOME", &home)
            .env("PATH", &path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("lilo daemon start spawns")?;
        wait_for_socket(&socket, &mut child, timeout)?;
        Ok(Self {
            child: Some(child),
            lilo,
            home,
            socket,
            path,
            timeout,
        })
    }

    pub fn command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.base_command();
        command.args(args);
        command
    }

    pub fn home_path(&self) -> &Path {
        &self.home
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    pub fn stop(&mut self) {
        let _ = self
            .command(["daemon", "stop"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + self.timeout;
            while Instant::now() < deadline {
                if child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn base_command(&self) -> Command {
        let mut command = Command::new(&self.lilo);
        command
            .env_remove("CLAUDE_CONFIG_DIR")
            .env("LILO_HOME", &self.home)
            .env("LILO_SOCKET_PATH", &self.socket)
            .env("HOME", &self.home)
            .env("PATH", &self.path);
        command
    }
}

impl Drop for LiloDaemon {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn lilo_bin() -> PathBuf {
    if let Some(path) = std::env::var_os("LILO_TEST_BIN") {
        return PathBuf::from(path);
    }
    assert_cmd::cargo::cargo_bin("lilo")
}

pub fn fake_runtime_path(command: &str) -> Result<TempDir> {
    let dir = tempfile::tempdir().context("runtime path tempdir creates")?;
    write_fake_runtime(dir.path(), command)?;
    Ok(dir)
}

pub fn write_fake_runtime(dir: &Path, command: &str) -> Result<PathBuf> {
    write_fake_command(dir, command, FAKE_RUNTIME_SCRIPT)
}

pub fn write_fake_command(dir: &Path, command: &str, script: &str) -> Result<PathBuf> {
    let runtime = dir.join(command);
    std::fs::write(&runtime, script).context("fake command writes")?;

    let mut permissions = std::fs::metadata(&runtime)
        .context("fake command metadata")?
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&runtime, permissions).context("fake command is executable")?;
    Ok(runtime)
}

pub fn test_path(prefix: Option<&Path>) -> Result<OsString> {
    let prefixes = prefix.into_iter().map(Path::to_path_buf);
    let paths = prefixes.chain(
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>()),
    );
    std::env::join_paths(paths).context("PATH can be joined")
}

pub fn wait_for_socket(socket: &Path, child: &mut Child, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let mut last_error = None;
    while Instant::now() < deadline {
        match UnixStream::connect(socket) {
            Ok(_) => return Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::NotFound | ErrorKind::ConnectionRefused
                ) =>
            {
                last_error = Some(error);
            }
            Err(error) => return Err(error).context("daemon socket connect failed"),
        }
        if let Some(status) = child.try_wait()? {
            bail!(
                "daemon exited before socket accepted connections: {status}\nstderr:\n{}",
                daemon_stderr(child)
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    bail!(
        "daemon socket did not accept connections at {}; last error={last_error:?}",
        socket.display()
    )
}

pub fn assert_success(command: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{command} failed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn daemon_stderr(child: &mut Child) -> String {
    let Some(stderr) = child.stderr.as_mut() else {
        return String::new();
    };
    let mut contents = String::new();
    let _ = stderr.read_to_string(&mut contents);
    contents
}
