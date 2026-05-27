use anyhow::{Result, bail};
use lilo_db::LiloDb;
use lilo_session_core::SmPaths;

use crate::run_daemon_with_db;

#[derive(Clone)]
pub struct SessionServiceContext {
    paths: SmPaths,
    db: LiloDb,
}

impl SessionServiceContext {
    pub fn new(paths: SmPaths, db: LiloDb) -> Self {
        Self { paths, db }
    }

    pub async fn from_env() -> Result<Self> {
        let paths = SmPaths::from_env()?;
        let db = LiloDb::open_path(&paths.database).await?;
        Ok(Self::new(paths, db))
    }

    pub fn paths(&self) -> &SmPaths {
        &self.paths
    }

    pub fn into_parts(self) -> (SmPaths, LiloDb) {
        (self.paths, self.db)
    }
}

#[derive(Clone)]
pub struct SessionService {
    paths: SmPaths,
    db: LiloDb,
}

impl SessionService {
    pub fn build(ctx: SessionServiceContext) -> Result<Self> {
        let (paths, db) = ctx.into_parts();
        validate_paths(&paths)?;
        Ok(Self { paths, db })
    }

    pub fn paths(&self) -> &SmPaths {
        &self.paths
    }

    pub async fn run(self) -> Result<()> {
        run_daemon_with_db(self.paths, self.db).await
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

    use lilo_db::LiloDb;
    use lilo_session_core::SmPaths;

    use super::{SessionService, SessionServiceContext};

    #[tokio::test]
    async fn build_preserves_session_paths_for_later_composition() {
        let paths = SmPaths::new(PathBuf::from("/tmp/lilo-session-service-test"));
        let db = LiloDb::open_path(&paths.database).await.expect("open db");

        let service = SessionService::build(SessionServiceContext::new(paths.clone(), db))
            .expect("build session service");

        assert_eq!(service.paths(), &paths);
    }
}
