use std::collections::{HashMap, HashSet};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use lilo_db::test_support::TestDb;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_sys::process::ProcessStartTime;

use crate::{DaemonConfig, ReconcileConfig, RuntimeServiceContext, reconcile::ProcessProbe};

pub(crate) struct FakeProcessProbe {
    alive: HashSet<u32>,
    start_times: HashMap<u32, DateTime<Utc>>,
}

impl FakeProcessProbe {
    pub(crate) fn new(
        alive: impl IntoIterator<Item = u32>,
        start_times: impl IntoIterator<Item = (u32, DateTime<Utc>)>,
    ) -> Self {
        Self {
            alive: alive.into_iter().collect(),
            start_times: start_times.into_iter().collect(),
        }
    }
}

impl ProcessProbe for FakeProcessProbe {
    fn pid_alive(&self, pid: u32) -> bool {
        self.alive.contains(&pid)
    }

    fn start_time_for_pid(&self, pid: u32) -> Result<ProcessStartTime> {
        Ok(self
            .start_times
            .get(&pid)
            .copied()
            .map_or(ProcessStartTime::Unsupported, ProcessStartTime::Known))
    }
}

pub(crate) struct ChildGuard(Child);

impl ChildGuard {
    pub(crate) fn spawn() -> Self {
        Self(
            Command::new("sleep")
                .arg("60")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn child"),
        )
    }

    pub(crate) fn id(&self) -> u32 {
        self.0.id()
    }

    pub(crate) fn start_time(&self) -> DateTime<Utc> {
        match lilo_sys::process::start_time_probe_for_pid(self.id()).expect("child start time") {
            ProcessStartTime::Known(start_time) => start_time,
            ProcessStartTime::Gone => panic!("child exited before its start time was read"),
            ProcessStartTime::Unsupported => panic!("child start time is unsupported"),
        }
    }

    pub(crate) fn is_alive(&mut self) -> bool {
        self.0.try_wait().expect("child status").is_none()
    }

    pub(crate) fn kill_and_wait(&mut self) {
        let _ = self.0.kill();
        self.0.wait().expect("wait for child");
    }

    pub(crate) fn wait_for_exit(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !self.is_alive() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        self.kill_and_wait();
        panic!("child was still alive after force preemption");
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) struct RuntimeServiceFixture {
    pub(crate) dir: tempfile::TempDir,
    pub(crate) config: DaemonConfig,
    pub(crate) testdb: TestDb,
}

impl RuntimeServiceFixture {
    pub(crate) async fn new(reconcile: ReconcileConfig) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = LiloPaths::new(LiloHome::from_path(dir.path().join("lilo")).expect("home"));
        let config = DaemonConfig {
            endpoint: lilo_paths::RuntimeEndpoint::unix_socket(paths.socket_path()),
            shim_path: dir.path().join("shim"),
            log_root: paths.logs_root(),
            data_root: paths.data_root(),
            reconcile,
            docker_preflight: crate::docker_preflight::DockerPreflightConfig::default(),
            tmux_server_label: None,
        };
        install_fake_shim(&config.shim_path);
        let testdb = TestDb::create().await.expect("db");

        Self {
            dir,
            config,
            testdb,
        }
    }

    pub(crate) fn context(&self) -> RuntimeServiceContext {
        RuntimeServiceContext::new(self.config.clone(), self.testdb.db().clone())
    }

    pub(crate) async fn cleanup(self) {
        self.testdb.cleanup().await.expect("test db cleans up");
    }
}

fn install_fake_shim(path: &Path) {
    std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("fake shim");
    let mut permissions = std::fs::metadata(path)
        .expect("fake shim metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("fake shim permissions");
}
