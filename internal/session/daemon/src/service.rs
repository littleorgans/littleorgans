use anyhow::{Result, bail};
use lilo_session_core::SmPaths;

use crate::run_daemon;

#[derive(Clone, Debug)]
pub struct SessionServiceContext {
    paths: SmPaths,
}

impl SessionServiceContext {
    pub fn new(paths: SmPaths) -> Self {
        Self { paths }
    }

    pub fn from_env() -> Result<Self> {
        Ok(Self::new(SmPaths::from_env()?))
    }

    pub fn paths(&self) -> &SmPaths {
        &self.paths
    }

    pub fn into_paths(self) -> SmPaths {
        self.paths
    }
}

impl From<SmPaths> for SessionServiceContext {
    fn from(paths: SmPaths) -> Self {
        Self::new(paths)
    }
}

#[derive(Clone, Debug)]
pub struct SessionService {
    paths: SmPaths,
}

impl SessionService {
    pub fn build(ctx: SessionServiceContext) -> Result<Self> {
        let paths = ctx.into_paths();
        validate_paths(&paths)?;
        Ok(Self { paths })
    }

    pub fn paths(&self) -> &SmPaths {
        &self.paths
    }

    pub async fn run(self) -> Result<()> {
        run_daemon(self.paths).await
    }
}

fn validate_paths(paths: &SmPaths) -> Result<()> {
    if paths.dir.as_os_str().is_empty() {
        bail!("session home directory cannot be empty");
    }
    if paths.pidfile.as_os_str().is_empty() {
        bail!("session pidfile path cannot be empty");
    }
    if paths.database.as_os_str().is_empty() {
        bail!("session database path cannot be empty");
    }
    if paths.log.as_os_str().is_empty() {
        bail!("session log path cannot be empty");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use lilo_session_core::SmPaths;

    use super::{SessionService, SessionServiceContext};

    #[test]
    fn build_preserves_session_paths_for_later_composition() {
        let paths = SmPaths::new(PathBuf::from("/tmp/lilo-session-service-test"));

        let service = SessionService::build(SessionServiceContext::new(paths.clone()))
            .expect("build session service");

        assert_eq!(service.paths(), &paths);
    }
}
