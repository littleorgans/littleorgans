use std::path::Path;

use anyhow::{Context, Result, bail};

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
