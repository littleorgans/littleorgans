use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use lilo_paths::RuntimeEndpoint;

pub fn socket_path_from_env() -> Result<PathBuf> {
    Ok(lilo_paths::unix_socket_path_from_env()?)
}

pub fn runtime_endpoint_from_env() -> Result<RuntimeEndpoint> {
    Ok(lilo_paths::runtime_endpoint_from_env()?)
}

pub fn display_socket_path(path: &Path) -> String {
    lilo_paths::display_unix_socket_path_from_env(path)
}

pub fn prepare_socket(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("socket path {} has no parent", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;

    remove_socket_file(path)?;
    Ok(())
}

pub fn remove_socket_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => bail!("failed to remove {}: {error}", path.display()),
    }
}
