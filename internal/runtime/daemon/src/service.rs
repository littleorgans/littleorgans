use anyhow::Result;
use lilo_db::LiloDb;

use crate::{DaemonConfig, server::run_daemon_with_db};

#[derive(Clone)]
pub struct RuntimeServiceContext {
    config: DaemonConfig,
    db: LiloDb,
}

impl RuntimeServiceContext {
    pub fn new(config: DaemonConfig, db: LiloDb) -> Self {
        Self { config, db }
    }

    pub async fn from_env() -> Result<Self> {
        let config = DaemonConfig::from_env()?;
        let db = LiloDb::open_path(&config.store.db_path).await?;
        Ok(Self::new(config, db))
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub fn into_parts(self) -> (DaemonConfig, LiloDb) {
        (self.config, self.db)
    }
}

#[derive(Clone)]
pub struct RuntimeService {
    config: DaemonConfig,
    db: LiloDb,
}

impl RuntimeService {
    pub fn build(ctx: RuntimeServiceContext) -> Result<Self> {
        let (config, db) = ctx.into_parts();
        let _ = config.socket_path()?;
        Ok(Self { config, db })
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub async fn run(self) -> Result<()> {
        run_daemon_with_db(self.config, self.db).await
    }
}

#[cfg(test)]
mod tests {
    use lilo_paths::RuntimeEndpoint;
    use lilo_runtime_store::StoreConfig;

    use crate::{ReconcileConfig, docker_preflight::DockerPreflightConfig};

    use super::{RuntimeService, RuntimeServiceContext};
    use crate::DaemonConfig;

    #[tokio::test]
    async fn build_preserves_daemon_config_for_later_composition() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        let config = DaemonConfig {
            endpoint: RuntimeEndpoint::unix_socket(tempdir.path().join("rtm.sock")),
            shim_path: tempdir.path().join("rtm-shim"),
            log_root: tempdir.path().join("logs"),
            store: StoreConfig {
                db_path: tempdir.path().join("rtm.db"),
            },
            reconcile: ReconcileConfig::default(),
            docker_preflight: DockerPreflightConfig::new(
                "runtime-matters-agent:latest",
                false,
                false,
            ),
        };

        let db = lilo_db::LiloDb::open_path(&config.store.db_path)
            .await
            .expect("open lilo db");
        let service = RuntimeService::build(RuntimeServiceContext::new(config.clone(), db))
            .expect("build runtime service");

        assert_eq!(
            service.config().socket_path().expect("service socket path"),
            config.socket_path().expect("config socket path")
        );
    }
}
