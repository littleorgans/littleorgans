use std::path::PathBuf;

use anyhow::Result;
use lilo_paths::{LiloHome, LiloPaths, RuntimeEndpoint};
use lilo_runtime_store::StoreConfig;
use uuid::Uuid;

use crate::{docker_preflight::DockerPreflightConfig, reconcile};

#[derive(Clone, Debug)]
pub struct DaemonConfig {
    pub endpoint: RuntimeEndpoint,
    pub shim_path: PathBuf,
    pub log_root: PathBuf,
    pub store: StoreConfig,
    pub reconcile: reconcile::ReconcileConfig,
    pub docker_preflight: DockerPreflightConfig,
}

impl DaemonConfig {
    pub fn from_env() -> Result<Self> {
        let home = LiloHome::from_env()?;
        let paths = LiloPaths::new(home);
        Self::from_lilo_paths(&paths)
    }

    pub fn from_lilo_paths(paths: &LiloPaths) -> Result<Self> {
        Ok(Self {
            endpoint: RuntimeEndpoint::unix_socket(paths.socket_path()),
            shim_path: lilo_paths::shim_path_from_env()?,
            log_root: paths.logs_root().join("runtimes"),
            store: StoreConfig {
                db_path: paths.db_path(),
            },
            reconcile: reconcile::ReconcileConfig::from_env()?,
            docker_preflight: DockerPreflightConfig::from_env(),
        })
    }

    #[cfg(test)]
    pub(crate) fn test_fixture() -> Self {
        Self::test_fixture_with_docker_preflight(DockerPreflightConfig::default())
    }

    #[cfg(test)]
    pub(crate) fn test_fixture_with_docker_preflight(
        docker_preflight: DockerPreflightConfig,
    ) -> Self {
        Self {
            endpoint: RuntimeEndpoint::unix_socket("/tmp/rtm.sock"),
            shim_path: PathBuf::from("/tmp/rtm-shim"),
            log_root: PathBuf::from("/tmp/rtm/logs"),
            store: StoreConfig {
                db_path: PathBuf::from("/tmp/rtm.db"),
            },
            reconcile: reconcile::ReconcileConfig::default(),
            docker_preflight,
        }
    }

    pub fn socket_path(&self) -> Result<&std::path::Path> {
        Ok(self.endpoint.unix_socket_path()?)
    }

    pub fn session_log_dir(&self, session_id: Uuid) -> PathBuf {
        self.log_root.join(session_id.to_string())
    }

    pub fn session_log_paths(&self, session_id: Uuid) -> crate::shim_socket::HeadlessLogPaths {
        let log_dir = self.session_log_dir(session_id);
        crate::shim_socket::HeadlessLogPaths {
            stdout_path: log_dir.join("stdout.log"),
            stderr_path: log_dir.join("stderr.log"),
            log_dir,
        }
    }

    pub fn data_dir(&self) -> PathBuf {
        self.store
            .db_path
            .parent()
            .map_or_else(|| self.log_root.clone(), PathBuf::from)
    }
}
