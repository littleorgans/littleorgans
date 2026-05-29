use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::{DaemonConfig, ReconcileConfig, RuntimeServiceContext};
use lilo_db::LiloDb;
use lilo_paths::{LiloHome, LiloPaths};
use lilo_runtime_store::StoreConfig;

pub(crate) struct RuntimeServiceFixture {
    pub(crate) dir: tempfile::TempDir,
    pub(crate) config: DaemonConfig,
    pub(crate) db: LiloDb,
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
        };
        install_fake_shim(&config.shim_path);
        let db = LiloDb::open(&paths).await.expect("db");

        Self { dir, config, db }
    }

    pub(crate) fn context(&self) -> RuntimeServiceContext {
        RuntimeServiceContext::new(self.config.clone(), self.db.clone())
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
