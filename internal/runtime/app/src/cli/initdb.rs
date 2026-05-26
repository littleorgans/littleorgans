use anyhow::Result;
use lilo_runtime_store::{LifecycleStore, StoreConfig};

pub async fn run() -> Result<()> {
    let config = StoreConfig::from_env()?;
    let path = config.db_path.clone();
    LifecycleStore::open(config).await?;
    println!("rtm db initialized at {}", path.display());
    Ok(())
}
