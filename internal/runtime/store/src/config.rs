use std::path::PathBuf;

use anyhow::Result;
use lilo_paths::{LiloHome, LiloPaths};

#[derive(Clone, Debug)]
pub struct StoreConfig {
    pub db_path: PathBuf,
}

impl StoreConfig {
    pub fn from_env() -> Result<Self> {
        let home = LiloHome::from_env()?;
        let db_path = LiloPaths::new(home).db_path();
        Ok(Self { db_path })
    }
}
