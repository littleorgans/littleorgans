use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use lilo_session_core::{Namespace, SpawnRequest};
use lilo_session_store::SqliteStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpawnLocation {
    pub namespace: Namespace,
    pub dir: PathBuf,
}

pub(crate) fn normalize_spawn_request(
    request: &mut SpawnRequest,
    store: &SqliteStore,
) -> Result<SpawnLocation> {
    let dir = request
        .dir
        .clone()
        .unwrap_or_else(|| request.workspace.clone());
    validate_dir(&dir)?;

    let namespace = request.namespace.clone().unwrap_or_default();
    let exists = store
        .namespace_exists(&namespace)
        .context("failed to validate namespace")?;
    if !exists {
        return Err(anyhow!("namespace not found: {namespace}"));
    }

    request.workspace.clone_from(&dir);
    let dir = PathBuf::from(dir);
    Ok(SpawnLocation { namespace, dir })
}

fn validate_dir(dir: &str) -> Result<()> {
    if dir.is_empty() {
        bail!("dir must not be empty");
    }
    let path = std::path::Path::new(dir);
    if !path.is_absolute() {
        bail!("dir must be an absolute path; got {dir} (resolve relative paths in the caller)");
    }
    if !path.is_dir() {
        bail!("dir must point to an existing directory: {dir}");
    }
    Ok(())
}
