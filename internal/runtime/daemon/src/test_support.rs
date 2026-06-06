use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::{DaemonConfig, ReconcileConfig, RuntimeServiceContext};
use lilo_db::test_support::TestDb;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_runtime_store::StoreConfig;

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
            store: StoreConfig {
                db_path: paths.db_path(),
            },
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
