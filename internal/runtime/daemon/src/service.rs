use anyhow::Result;

use crate::{DaemonConfig, run_daemon};

#[derive(Clone, Debug)]
pub struct RuntimeServiceContext {
    config: DaemonConfig,
}

impl RuntimeServiceContext {
    pub fn new(config: DaemonConfig) -> Self {
        Self { config }
    }

    pub fn from_env() -> Result<Self> {
        Ok(Self::new(DaemonConfig::from_env()?))
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub fn into_config(self) -> DaemonConfig {
        self.config
    }
}

impl From<DaemonConfig> for RuntimeServiceContext {
    fn from(config: DaemonConfig) -> Self {
        Self::new(config)
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeService {
    config: DaemonConfig,
}

impl RuntimeService {
    pub fn build(ctx: RuntimeServiceContext) -> Result<Self> {
        let config = ctx.into_config();
        let _ = config.socket_path()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub async fn run(self) -> Result<()> {
        run_daemon(self.config).await
    }
}

#[cfg(test)]
mod tests {
    use lilo_paths::RuntimeEndpoint;
    use lilo_runtime_store::StoreConfig;

    use crate::{ReconcileConfig, docker_preflight::DockerPreflightConfig};

    use super::{RuntimeService, RuntimeServiceContext};
    use crate::DaemonConfig;

    #[test]
    fn build_preserves_daemon_config_for_later_composition() {
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

        let service = RuntimeService::build(RuntimeServiceContext::new(config.clone()))
            .expect("build runtime service");

        assert_eq!(
            service.config().socket_path().expect("service socket path"),
            config.socket_path().expect("config socket path")
        );
    }
}
