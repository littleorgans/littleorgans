use std::path::PathBuf;

use anyhow::Result;

#[derive(Clone, Debug)]
pub struct StoreConfig {
    pub db_path: PathBuf,
}

impl StoreConfig {
    pub fn from_env() -> Result<Self> {
        let db_path = lilo_paths::db_path_from_env()?;
        Ok(Self { db_path })
    }
}
